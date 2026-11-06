//! Plugin discovery: `plugins/*/plugin.toml` through the [`ScriptFiles`] port (§9.4 step 1).

use std::collections::BTreeSet;

use pkrs_core::{PluginManifest, is_plugin_id};

use super::error::PluginError;
use crate::host::ScriptFiles;

/// Folder that holds every plugin (`plugins/<id>/`).
pub const PLUGINS_DIR: &str = "plugins";

/// Project-relative path of `id`'s manifest (`plugins/<id>/plugin.toml`).
#[must_use]
pub fn manifest_path(id: &str) -> String {
    format!("{PLUGINS_DIR}/{id}/{}", PluginManifest::FILE_NAME)
}

/// Project-relative path of `id`'s entry script (`plugins/<id>/init.lua`).
#[must_use]
pub fn init_path(id: &str) -> String {
    format!("{PLUGINS_DIR}/{id}/init.lua")
}

/// Result of scanning `plugins/`.
#[derive(Debug, Default)]
pub struct Discovery {
    /// Valid manifests, sorted by id.
    pub manifests: Vec<PluginManifest>,
    /// Problems with *enabled* plugins' manifests. Broken manifests of plugins that are not
    /// enabled are only logged, so an unused folder cannot stop the game.
    pub errors: Vec<PluginError>,
}

/// Scans `plugins/*/plugin.toml`. Folders come from [`ScriptFiles::list_dirs`], plus every id
/// in `enabled` (so sources that cannot list still find enabled plugins). A folder must be
/// named after its manifest's `[plugin].id`.
pub fn discover_plugins(files: &dyn ScriptFiles, enabled: &[String]) -> Discovery {
    let enabled_set: BTreeSet<&str> = enabled.iter().map(String::as_str).collect();
    let mut folders: BTreeSet<String> = files.list_dirs(PLUGINS_DIR).into_iter().collect();
    folders.extend(enabled.iter().filter(|id| is_plugin_id(id)).cloned());

    let mut discovery = Discovery::default();
    for folder in folders {
        if !is_plugin_id(&folder) {
            tracing::debug!(target: "script", folder, "skipping folder that is not a plugin id");
            continue;
        }
        let path = manifest_path(&folder);
        if !files.exists(&path) {
            // An enabled plugin without a manifest is reported as `NotFound` by the resolver.
            continue;
        }
        let manifest = files
            .read_script(&path)
            .map_err(|e| PluginError::Read {
                plugin: folder.clone(),
                path: path.clone(),
                reason: e.to_string(),
            })
            .and_then(|src| {
                PluginManifest::from_toml_str(&src).map_err(|source| PluginError::Manifest {
                    plugin: folder.clone(),
                    source,
                })
            });
        let manifest = match manifest {
            Ok(m) if m.id() != folder => Err(PluginError::IdMismatch {
                folder: folder.clone(),
                id: m.id().to_owned(),
            }),
            other => other,
        };
        match manifest {
            Ok(m) => discovery.manifests.push(m),
            Err(err) if enabled_set.contains(folder.as_str()) => discovery.errors.push(err),
            Err(err) => tracing::warn!(target: "script", "ignoring disabled plugin: {err}"),
        }
    }
    discovery
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryScripts;

    fn toml(id: &str) -> String {
        format!("[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n")
    }

    fn ids(d: &Discovery) -> Vec<&str> {
        d.manifests.iter().map(PluginManifest::id).collect()
    }

    #[test]
    fn finds_every_plugin_folder() {
        let files = MemoryScripts::new()
            .with("plugins/day-night/plugin.toml", toml("day-night"))
            .with("plugins/resonance/plugin.toml", toml("resonance"))
            .with("plugins/resonance/init.lua", "")
            .with("plugins/no-manifest/init.lua", "");
        let d = discover_plugins(&files, &[]);
        assert_eq!(ids(&d), ["day-night", "resonance"]);
        assert!(d.errors.is_empty());
    }

    #[test]
    fn broken_manifests_only_matter_when_enabled() {
        let files = MemoryScripts::new()
            .with("plugins/broken/plugin.toml", "[plugin]\nid = 3\n")
            .with("plugins/renamed/plugin.toml", toml("other"))
            .with("plugins/good/plugin.toml", toml("good"));
        let d = discover_plugins(&files, &[]);
        assert_eq!(ids(&d), ["good"]);
        assert!(d.errors.is_empty());

        let d = discover_plugins(&files, &["broken".into(), "renamed".into()]);
        assert_eq!(ids(&d), ["good"]);
        let messages: Vec<String> = d.errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].starts_with("plugin `broken`: plugin.toml: "));
        assert_eq!(
            messages[1],
            "plugin folder `plugins/renamed` declares id `other`; the folder must be named after the id"
        );
    }

    /// A source that cannot list directories still finds enabled plugins.
    struct NoListing(MemoryScripts);

    impl ScriptFiles for NoListing {
        fn read_script(&self, path: &str) -> crate::Result<String> {
            self.0.read_script(path)
        }
        fn exists(&self, path: &str) -> bool {
            self.0.exists(path)
        }
    }

    #[test]
    fn enabled_plugins_are_found_without_listing() {
        let files = NoListing(
            MemoryScripts::new()
                .with("plugins/a/plugin.toml", toml("a"))
                .with("plugins/b/plugin.toml", toml("b")),
        );
        let d = discover_plugins(&files, &["b".into()]);
        assert_eq!(ids(&d), ["b"]);
    }

    #[test]
    fn paths() {
        assert_eq!(manifest_path("x"), "plugins/x/plugin.toml");
        assert_eq!(init_path("x"), "plugins/x/init.lua");
    }
}
