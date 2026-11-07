//! `pkrs check` (§2.3): validates `pkrs.toml` and the plugin set; with `--plugins` it also
//! merges the content `data/` folders and loads the plugins in a sandboxed script host, and
//! returns the resolved hook table and any content errors (§9.4).
//!
//! Further checks (RON data, Tiled maps, Lua syntax of game scripts) arrive with M7.

use std::fmt;
use std::ops::RangeInclusive;
use std::path::Path;

use pkrs_assets::{EntryKind, FolderVfs, Vfs, VfsPath};
use pkrs_core::{ManifestError, Pcg64Rng, ProjectManifest, Version};
use pkrs_script::{
    ContentError, HookTable, LuaScriptHost, PluginErrors, PluginInitError, ScriptError,
    ScriptFiles, ScriptLimits, load_plugins, plan_plugins,
};

/// Scripting API levels this CLI checks against (same as the runtime, §9.7).
pub const SUPPORTED_API: RangeInclusive<u32> = 1..=1;

/// Seed of the throwaway RNG plugins see while `check --plugins` loads them.
const CHECK_SEED: u64 = 0;

/// Why `pkrs check` failed.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// `pkrs.toml` is missing, malformed or targets another engine.
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    /// The engine version could not be parsed.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// The project folder cannot be mounted.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// The enabled plugin set is invalid (missing, cycle, conflict, version, API level).
    #[error(transparent)]
    Plugins(#[from] PluginErrors),
    /// The script host could not be created.
    #[error(transparent)]
    Script(#[from] ScriptError),
}

/// What `pkrs check` found.
#[derive(Debug, Default)]
pub struct CheckReport {
    /// Enabled plugins in resolved load order.
    pub load_order: Vec<String>,
    /// Plugins whose `init.lua` failed (only with `--plugins`).
    pub failed: Vec<PluginInitError>,
    /// Problems in `data/` folders: duplicate ids without `override`, parse and schema
    /// errors (only with `--plugins`, §9.4).
    pub content_errors: Vec<ContentError>,
    /// The resolved hook table (only with `--plugins`).
    pub hooks: Option<HookTable>,
}

impl CheckReport {
    /// `true` when nothing failed.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.failed.is_empty() && self.content_errors.is_empty()
    }
}

/// The stdout part of `pkrs check`: the resolved load order, then (with `--plugins`) the hook
/// table. Failures are not included; the CLI prints them to stderr.
///
/// ```text
/// plugins (load order): shield, crit
/// battle hooks:
///   on_damage_taken
///       -5  shield
///       10  crit
/// world events:
///   (none)
/// ```
impl fmt::Display for CheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.load_order.is_empty() {
            writeln!(f, "plugins: none enabled")?;
        } else {
            writeln!(f, "plugins (load order): {}", self.load_order.join(", "))?;
        }
        match &self.hooks {
            Some(hooks) => write!(f, "{hooks}"),
            None => Ok(()),
        }
    }
}

/// Checks the project at `dir`. With `plugins`, also runs every enabled plugin's `init.lua`
/// (sandboxed, §9.6) and collects the hook table they register.
pub fn check_project(dir: &Path, plugins: bool) -> Result<CheckReport, CheckError> {
    let manifest = ProjectManifest::load(dir)?;
    let engine = Version::parse(env!("CARGO_PKG_VERSION"))?;
    manifest.check_engine(&engine, SUPPORTED_API)?;
    let files = FolderScripts(FolderVfs::new(dir)?);
    let plan = plan_plugins(&files, &manifest.plugins.enabled, SUPPORTED_API)?;
    let mut report = CheckReport {
        load_order: plan.ids().iter().map(|s| (*s).to_owned()).collect(),
        ..CheckReport::default()
    };
    if plugins {
        let mut host = LuaScriptHost::new(
            Box::new(FolderScripts(FolderVfs::new(dir)?)),
            Box::new(Pcg64Rng::from_seed(CHECK_SEED)),
            ScriptLimits::default(),
        )?;
        let loaded = load_plugins(&mut host, &files, &plan);
        report.failed = loaded.failed;
        report.content_errors = loaded.content_errors;
        report.hooks = Some(host.hook_table());
    }
    Ok(report)
}

/// [`ScriptFiles`] over the project folder (read-only, through the assets VFS).
struct FolderScripts(FolderVfs);

impl ScriptFiles for FolderScripts {
    fn read_script(&self, path: &str) -> pkrs_script::Result<String> {
        let load_error = |reason: String| ScriptError::Load {
            path: path.to_owned(),
            reason,
        };
        let vfs_path = VfsPath::new(path).map_err(|e| load_error(e.to_string()))?;
        let bytes = self
            .0
            .read(&vfs_path)
            .map_err(|e| load_error(e.to_string()))?;
        String::from_utf8(bytes).map_err(|_| load_error("script is not valid UTF-8".into()))
    }

    fn exists(&self, path: &str) -> bool {
        VfsPath::new(path).is_ok_and(|p| self.0.exists(&p))
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        let Ok(dir) = VfsPath::new(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = self
            .0
            .list(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::Dir)
            .filter_map(|entry| entry.path.file_name().map(str::to_owned))
            .collect();
        names.sort();
        names
    }

    fn list_files(&self, dir: &str) -> Vec<String> {
        let Ok(dir) = VfsPath::new(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = self
            .0
            .list(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == EntryKind::File)
            .filter_map(|entry| entry.path.file_name().map(str::to_owned))
            .collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pkrs-check-{tag}-{}-{nanos}", std::process::id()));
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn manifest(enabled: &str) -> String {
        format!(
            "[game]\nname = \"T\"\nid = \"com.example.t\"\nversion = \"0.1.0\"\n\
             [engine]\nversion = \"{}\"\napi = 1\n[start]\nmap = \"start\"\nposition = [0, 0]\n\
             [plugins]\nenabled = [{enabled}]\n",
            env!("CARGO_PKG_VERSION")
        )
    }

    fn plugin(id: &str) -> String {
        format!("[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n")
    }

    #[test]
    fn reports_load_order_and_hook_table() {
        let dir = temp_project(
            "ok",
            &[
                ("pkrs.toml", &manifest("\"b\", \"a\"")),
                ("plugins/a/plugin.toml", &plugin("a")),
                (
                    "plugins/a/init.lua",
                    "battle.on('on_damage_taken', function() end, { priority = 5 })",
                ),
                ("plugins/b/plugin.toml", &plugin("b")),
                (
                    "plugins/b/init.lua",
                    "battle.on('on_damage_taken', function() end)\n\
                     battle.on('on_turn_start', function() end, { priority = -1 })",
                ),
            ],
        );
        let without = check_project(&dir, false).unwrap();
        assert_eq!(without.load_order, ["b", "a"]);
        assert!(without.hooks.is_none());

        let report = check_project(&dir, true).unwrap();
        assert!(report.is_ok());
        assert_eq!(
            report.hooks.unwrap().to_string(),
            "battle hooks:\n  on_damage_taken\n       0  b\n       5  a\n  on_turn_start\n      -1  b\n\
             world events:\n  (none)\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn report_formatting() {
        let empty = CheckReport::default();
        assert_eq!(empty.to_string(), "plugins: none enabled\n");

        let order_only = CheckReport {
            load_order: vec!["b".into(), "a".into()],
            ..CheckReport::default()
        };
        assert_eq!(order_only.to_string(), "plugins (load order): b, a\n");

        let dir = temp_project(
            "fmt",
            &[
                ("pkrs.toml", &manifest("\"a\"")),
                ("plugins/a/plugin.toml", &plugin("a")),
                (
                    "plugins/a/init.lua",
                    "events.on('map_enter', function() end, { priority = 2 })",
                ),
            ],
        );
        let report = check_project(&dir, true).unwrap();
        assert_eq!(
            report.to_string(),
            "plugins (load order): a\nbattle hooks:\n  (none)\nworld events:\n  map_enter\n       2  a\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn scaffolded_plugin_passes_check_without_edits() {
        let dir = temp_project("scaffold", &[("pkrs.toml", &manifest("\"my-plugin\""))]);
        let vars = crate::plugin::PluginVars::for_plugin("My Plugin").unwrap();
        let out = crate::plugin::new_plugin(&dir, &vars).unwrap();
        assert!(out.enable_hint.is_none(), "already enabled");
        let report = check_project(&dir, true).unwrap();
        assert!(report.is_ok(), "{:?}", report.failed);
        assert_eq!(report.load_order, ["my-plugin"]);
        let table = report.hooks.unwrap().to_string();
        for line in [
            "  on_damage_calc\n       0  my-plugin\n",
            "  map_enter\n       0  my-plugin\n",
        ] {
            assert!(table.contains(line), "{table}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn failing_plugins_and_bad_plugin_sets_are_reported() {
        let dir = temp_project(
            "fail",
            &[
                ("pkrs.toml", &manifest("\"a\"")),
                ("plugins/a/plugin.toml", &plugin("a")),
                ("plugins/a/init.lua", "error('boom')"),
            ],
        );
        let report = check_project(&dir, true).unwrap();
        assert!(!report.is_ok());
        assert_eq!(report.failed[0].plugin(), "a");
        std::fs::remove_dir_all(&dir).ok();

        let dir = temp_project("missing", &[("pkrs.toml", &manifest("\"ghost\""))]);
        assert!(matches!(
            check_project(&dir, false),
            Err(CheckError::Plugins(_))
        ));
        std::fs::remove_dir_all(dir).ok();
    }
}
