//! Plugin loader (§9.4, §9.7): discovery, manifest validation, dependency resolution, load
//! order and running each plugin's `init.lua` in its own `_ENV`.
//!
//! 1. [`discover_plugins`] scans `plugins/*/plugin.toml` through [`ScriptFiles`].
//! 2. [`resolve_load_order`] checks the plugins enabled in `pkrs.toml` (existence, API level,
//!    semver dependencies, conflicts, cycles) and sorts them deterministically.
//!    [`plan_plugins`] runs both steps and reports every problem at once.
//! 3. [`load_plugins`] binds each manifest to its plugin environment (`plugin.define()`) and
//!    runs `plugins/<id>/init.lua` in load order.
//!
//! Hook chains (combining handlers across plugins) are not part of this module.

mod discover;
mod error;
mod resolve;

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use pkrs_core::PluginManifest;

pub use discover::{Discovery, PLUGINS_DIR, discover_plugins, init_path, manifest_path};
pub use error::{PluginError, PluginErrors, PluginInitError};
pub use resolve::{PluginPlan, resolve_load_order};

use crate::error::Result;
use crate::host::{ScriptFiles, ScriptHost, ScriptOwner, ScriptValue};

/// A [`ScriptHost`] that can bind a plugin manifest to the plugin's environment.
pub trait PluginHost: ScriptHost {
    /// Installs the `plugin` table in `ScriptOwner::Plugin(manifest id)`'s `_ENV`.
    /// `plugin.define()` then returns the plugin's own table, bound to its manifest:
    /// `{ id, name, version, api, description, save = {} }`. Repeated calls return the same
    /// table. (`save` is the plugin's save namespace; persisting it arrives with §12.)
    fn bind_plugin(&mut self, manifest: &PluginManifest) -> Result<()>;
}

/// Discovers the plugins in `files` and resolves those in `enabled` (§9.4 steps 1–3).
/// Manifest problems of enabled plugins and resolution problems are reported together.
pub fn plan_plugins(
    files: &dyn ScriptFiles,
    enabled: &[String],
    supported_api: RangeInclusive<u32>,
) -> Result<PluginPlan, PluginErrors> {
    let discovery = discover_plugins(files, enabled);
    let broken: BTreeSet<String> = discovery
        .errors
        .iter()
        .filter_map(|e| match e {
            PluginError::Read { plugin, .. } | PluginError::Manifest { plugin, .. } => {
                Some(plugin.clone())
            }
            PluginError::IdMismatch { folder, .. } => Some(folder.clone()),
            _ => None,
        })
        .collect();
    let resolved = resolve_load_order(&discovery.manifests, enabled, supported_api);
    let mut errors = discovery.errors;
    match resolved {
        Ok(plan) if errors.is_empty() => Ok(plan),
        Ok(_) => Err(PluginErrors(errors)),
        Err(PluginErrors(more)) => {
            // Broken plugins are already reported; do not also call them "not found".
            errors.extend(more.into_iter().filter(
                |e| !matches!(e, PluginError::NotFound { plugin, .. } if broken.contains(plugin)),
            ));
            Err(PluginErrors(errors))
        }
    }
}

/// What [`load_plugins`] did.
#[derive(Debug, Default)]
pub struct PluginLoadReport {
    /// Plugins that initialised, in load order.
    pub loaded: Vec<String>,
    /// Plugins that failed, or were skipped because a dependency failed, in load order.
    pub failed: Vec<PluginInitError>,
}

/// Runs every plugin of `plan` in load order: binds its manifest ([`PluginHost::bind_plugin`])
/// and runs `plugins/<id>/init.lua` (if present; a plugin may ship data only) in its own
/// `_ENV`. A failing plugin never aborts the others (§9.6), but plugins that depend on it are
/// skipped.
pub fn load_plugins<H: PluginHost + ?Sized>(
    host: &mut H,
    files: &dyn ScriptFiles,
    plan: &PluginPlan,
) -> PluginLoadReport {
    let mut report = PluginLoadReport::default();
    let mut failed: BTreeSet<String> = BTreeSet::new();
    for manifest in plan.plugins() {
        let id = manifest.id();
        if let Some(dep) = manifest.dependencies.keys().find(|d| failed.contains(*d)) {
            tracing::warn!(target: "script", plugin = id, dependency = %dep, "plugin skipped: dependency failed");
            report.failed.push(PluginInitError::DependencyFailed {
                plugin: id.to_owned(),
                dependency: dep.clone(),
            });
            failed.insert(id.to_owned());
            continue;
        }
        match init_plugin(host, files, manifest) {
            Ok(()) => {
                tracing::info!(
                    target: "script",
                    plugin = id,
                    version = %manifest.plugin.version,
                    "plugin loaded"
                );
                report.loaded.push(id.to_owned());
            }
            Err(source) => {
                tracing::error!(target: "script", plugin = id, "plugin failed: {source}");
                report.failed.push(PluginInitError::Script {
                    plugin: id.to_owned(),
                    source,
                });
                failed.insert(id.to_owned());
            }
        }
    }
    report
}

fn init_plugin<H: PluginHost + ?Sized>(
    host: &mut H,
    files: &dyn ScriptFiles,
    manifest: &PluginManifest,
) -> Result<()> {
    host.bind_plugin(manifest)?;
    let path = init_path(manifest.id());
    if files.exists(&path) {
        let owner = ScriptOwner::plugin(manifest.id());
        let _: ScriptValue = host.run_file(&owner, &path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{MemoryScripts, ScriptLimits};
    use crate::lua::LuaScriptHost;
    use pkrs_core::Pcg64Rng;

    fn toml(id: &str, deps: &str) -> String {
        format!(
            "[plugin]\nid = \"{id}\"\nname = \"Plugin {id}\"\nversion = \"1.2.3\"\napi = 1\n\
             description = \"about {id}\"\n[dependencies]\n{deps}"
        )
    }

    fn host(files: &MemoryScripts) -> LuaScriptHost {
        LuaScriptHost::new(
            Box::new(files.clone()),
            Box::new(Pcg64Rng::from_seed(1)),
            ScriptLimits::default(),
        )
        .unwrap()
    }

    fn enabled(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn define_binds_the_manifest_in_each_env() {
        let files = MemoryScripts::new()
            .with("plugins/alpha/plugin.toml", toml("alpha", ""))
            .with(
                "plugins/alpha/init.lua",
                "local P = plugin.define()\nP.save.count = 1\nloaded = P.id\n\
                 same = plugin.define() == P\nreturn P",
            )
            .with("plugins/beta/plugin.toml", toml("beta", "alpha = \"1\"\n"))
            .with(
                "plugins/beta/init.lua",
                "local P = plugin.define()\nloaded = P.id .. ':' .. P.version .. ':' .. P.api\n\
                 info = P.name .. '/' .. P.description\nsaw_alpha = loaded_by_alpha",
            );
        let plan = plan_plugins(&files, &enabled(&["beta", "alpha"]), 1..=1).unwrap();
        assert_eq!(plan.ids(), ["alpha", "beta"]);
        let mut lua = host(&files);
        let report = load_plugins(&mut lua, &files, &plan);
        assert_eq!(report.loaded, ["alpha", "beta"]);
        assert!(report.failed.is_empty());

        let alpha = ScriptOwner::plugin("alpha");
        let beta = ScriptOwner::plugin("beta");
        assert_eq!(lua.global(&alpha, "loaded").unwrap(), "alpha".into());
        assert_eq!(lua.global(&alpha, "same").unwrap(), true.into());
        assert_eq!(lua.global(&beta, "loaded").unwrap(), "beta:1.2.3:1".into());
        assert_eq!(
            lua.global(&beta, "info").unwrap(),
            "Plugin beta/about beta".into()
        );
        // Separate `_ENV`s: no plugin sees another's globals, nor does the game.
        assert!(lua.global(&beta, "same").unwrap().is_nil());
        assert!(lua.global(&ScriptOwner::Game, "plugin").unwrap().is_nil());
    }

    #[test]
    fn failures_are_reported_and_skip_dependants() {
        let files = MemoryScripts::new()
            .with("plugins/base/plugin.toml", toml("base", ""))
            .with("plugins/base/init.lua", "error('boom')")
            .with("plugins/child/plugin.toml", toml("child", "base = \"1\"\n"))
            .with("plugins/child/init.lua", "ran = true")
            .with("plugins/data-only/plugin.toml", toml("data-only", ""));
        let plan = plan_plugins(&files, &enabled(&["child", "base", "data-only"]), 1..=1).unwrap();
        let mut lua = host(&files);
        let report = load_plugins(&mut lua, &files, &plan);
        assert_eq!(report.loaded, ["data-only"]);
        let failed: Vec<&str> = report.failed.iter().map(PluginInitError::plugin).collect();
        assert_eq!(failed, ["base", "child"]);
        assert!(
            report.failed[0]
                .to_string()
                .contains("plugins/base/init.lua:1: boom"),
            "{}",
            report.failed[0]
        );
        assert!(matches!(
            report.failed[1],
            PluginInitError::DependencyFailed { .. }
        ));
        assert!(
            lua.global(&ScriptOwner::plugin("child"), "ran")
                .unwrap()
                .is_nil()
        );
    }

    #[test]
    fn plan_reports_manifest_and_resolution_errors_together() {
        let files = MemoryScripts::new()
            .with("plugins/bad/plugin.toml", "[plugin]\nid = \"bad\"\n")
            .with("plugins/ok/plugin.toml", toml("ok", "ghost = \"1\"\n"));
        let err = plan_plugins(&files, &enabled(&["bad", "ok", "gone"]), 1..=1).unwrap_err();
        let messages: Vec<String> = err.errors().iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 3, "{messages:#?}");
        assert!(messages[0].starts_with("plugin `bad`: plugin.toml: "));
        assert!(messages[1].contains("plugin `gone` is enabled but"));
        assert!(messages[2].contains("depends on `ghost`"));
    }
}
