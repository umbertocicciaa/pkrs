//! `pkrs check` (§2.3, §8.2): validates the whole project without running the game, through
//! the shared [`validate_project`] (also run by the runtime at start-up):
//!
//! - `pkrs.toml` (parse, ranges, engine version and API level);
//! - the plugin graph and every enabled `plugin.toml` (§9.4), and the plugins' `data/`;
//! - every RON file of `data/`: parse (stage 1), schema (stage 2) and cross-references
//!   (stage 3: types, abilities, moves, species, items, statuses, AI profiles, sprite paths,
//!   `text:` keys); battle data is skipped with `features.battles = false` (§8.1);
//! - every `maps/*.tmx` (Tiled layer conventions, warps, object scripts, sprites, items,
//!   encounter tables, sign text) and `maps/world.ron`;
//! - every Lua file of `scripts/` and `plugins/`, compiled but never run.
//!
//! Warnings: `events.on("tick")` listeners, `once` triggers whose script never sets a flag or
//! variable (§6.5), untranslated text keys and undefined `"text:…"` literals in Lua.
//!
//! With `--plugins` the enabled plugins' `init.lua` also run in a sandboxed script host: the
//! resolved hook table is printed, a failing plugin is an error, and content they register
//! with `data.register_*` takes part in the cross-reference checks.

use std::fmt;
use std::ops::RangeInclusive;
use std::path::Path;

use pkrs_assets::{EntryKind, FolderVfs, Vfs, VfsPath};
use pkrs_core::{Diagnostics, Pcg64Rng, ProjectManifest, Version};
use pkrs_data::DataFiles;
use pkrs_script::validate::{ValidateOptions, plugin_init_diagnostic, validate_project};
use pkrs_script::{
    HookTable, LuaScriptHost, ScriptError, ScriptFiles, ScriptLimits, load_plugins, plan_plugins,
};
use pkrs_world::{Map, MapError, MapImporter, TiledImporter};

/// Scripting API levels this CLI checks against (same as the runtime, §9.7).
pub const SUPPORTED_API: RangeInclusive<u32> = 1..=1;

/// Seed of the throwaway RNG plugins see while `check --plugins` loads them.
const CHECK_SEED: u64 = 0;

/// Why `pkrs check` could not run at all (content problems are diagnostics instead).
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// The engine version could not be parsed.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// The project folder cannot be mounted.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// The script host could not be created.
    #[error(transparent)]
    Script(#[from] ScriptError),
}

/// What `pkrs check` found.
#[derive(Debug, Default)]
pub struct CheckReport {
    /// Enabled plugins in resolved load order (empty when the plugin set is invalid).
    pub load_order: Vec<String>,
    /// The resolved hook table (only with `--plugins`).
    pub hooks: Option<HookTable>,
    /// Every error and warning, in check order.
    pub diagnostics: Diagnostics,
}

impl CheckReport {
    /// `true` when there is no error.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.has_errors()
    }

    /// `true` when the check passes: no error, and no warning either with `deny_warnings`.
    #[must_use]
    pub fn passes(&self, deny_warnings: bool) -> bool {
        self.is_ok() && !(deny_warnings && self.diagnostics.warning_count() > 0)
    }
}

/// The stdout part of `pkrs check`: the resolved load order, then (with `--plugins`) the hook
/// table. Diagnostics are not included; the CLI renders them to stderr.
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
    let engine = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let files = FolderScripts(FolderVfs::new(dir)?);
    let vfs = FolderVfs::new(dir)?;
    let mut import = |path: &str| -> Result<Map, MapError> {
        let vfs_path = VfsPath::new(path).map_err(|e| MapError::Parse {
            path: path.to_owned(),
            message: e.to_string(),
        })?;
        TiledImporter::new().import(&vfs, &vfs_path)
    };
    let mut report = CheckReport::default();
    let validation = if plugins {
        let mut host = LuaScriptHost::new(
            Box::new(FolderScripts(FolderVfs::new(dir)?)),
            Box::new(Pcg64Rng::from_seed(CHECK_SEED)),
            ScriptLimits::default(),
        )?;
        // An invalid plugin set is reported by the validation below.
        let enabled = ProjectManifest::load(dir)
            .map(|m| m.plugins.enabled)
            .unwrap_or_default();
        let mut init_errors = Vec::new();
        if let Ok(plan) = plan_plugins(&files, &enabled, SUPPORTED_API) {
            let loaded = load_plugins(&mut host, &files, &plan);
            init_errors = loaded.failed.iter().map(plugin_init_diagnostic).collect();
            report.hooks = Some(host.hook_table());
        }
        let mut validation = host.with_content(|db| {
            let options = ValidateOptions {
                engine,
                supported_api: SUPPORTED_API,
                plugin_content: Some(db),
            };
            validate_project(&files, &mut import, &options)
        });
        validation.diagnostics.extend(init_errors);
        validation
    } else {
        let options = ValidateOptions {
            engine,
            supported_api: SUPPORTED_API,
            plugin_content: None,
        };
        validate_project(&files, &mut import, &options)
    };
    report.load_order = validation
        .plan
        .as_ref()
        .map(|p| p.ids().iter().map(|s| (*s).to_owned()).collect())
        .unwrap_or_default();
    report.diagnostics = validation.diagnostics;
    Ok(report)
}

/// Reads a project file for diagnostic snippets (`None` when it cannot be read).
#[must_use]
pub fn read_source(dir: &Path, path: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(path)).ok()
}

/// [`ScriptFiles`] and [`DataFiles`] over the project folder (read-only, through the assets
/// VFS).
struct FolderScripts(FolderVfs);

impl FolderScripts {
    fn names(&self, dir: &str, kind: EntryKind) -> Vec<String> {
        let Ok(dir) = VfsPath::new(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = self
            .0
            .list(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.kind == kind)
            .filter_map(|entry| entry.path.file_name().map(str::to_owned))
            .collect();
        names.sort();
        names
    }
}

impl DataFiles for FolderScripts {
    fn list_files(&self, dir: &str) -> Vec<String> {
        self.names(dir, EntryKind::File)
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        self.names(dir, EntryKind::Dir)
    }

    fn exists(&self, path: &str) -> bool {
        ScriptFiles::exists(self, path)
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        self.read_script(path).map_err(|e| e.to_string())
    }
}

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
        self.names(dir, EntryKind::Dir)
    }

    fn list_files(&self, dir: &str) -> Vec<String> {
        self.names(dir, EntryKind::File)
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

    fn codes(report: &CheckReport) -> Vec<&str> {
        report.diagnostics.iter().map(|d| d.code.as_str()).collect()
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
        assert!(report.passes(true), "{}", report.diagnostics);
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
        assert!(report.is_ok(), "{}", report.diagnostics);
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
    fn data_folder_is_loaded_and_battle_data_follows_the_feature() {
        let species = "(id: \"a\", types: [\"normal\"], hp: 1, attack: 1, defense: 1, speed: 1)";
        let project = |tag: &str, battles: &str| {
            let toml = format!("{}[features]\nbattles = {battles}\n", manifest(""));
            temp_project(
                tag,
                &[
                    ("pkrs.toml", &toml),
                    ("data/species/a.ron", species),
                    ("data/species/b.ron", species),
                    ("data/items/potion.ron", "(id: \"potion\")"),
                ],
            )
        };
        let dir = project("data-on", "true");
        let report = check_project(&dir, false).unwrap();
        assert!(!report.is_ok());
        let errors: Vec<&str> = report
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            errors,
            [
                "duplicate species `a` in `data/species/b.ron` (already defined in `data/species/a.ron`)"
            ]
        );
        std::fs::remove_dir_all(dir).ok();

        // Story-only: the broken battle data is not even read.
        let dir = project("data-off", "false");
        let report = check_project(&dir, false).unwrap();
        assert!(report.is_ok(), "{}", report.diagnostics);
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
        assert_eq!(codes(&report), ["plugin::init"]);
        assert_eq!(
            report.diagnostics.items()[0].file.as_deref(),
            Some("plugins/a/init.lua")
        );
        std::fs::remove_dir_all(&dir).ok();

        let dir = temp_project("missing", &[("pkrs.toml", &manifest("\"ghost\""))]);
        let report = check_project(&dir, false).unwrap();
        assert_eq!(codes(&report), ["plugin::not-found"]);
        assert!(report.load_order.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn warnings_only_fail_with_deny_warnings() {
        let dir = temp_project(
            "warn",
            &[
                ("pkrs.toml", &manifest("")),
                ("scripts/main.lua", "events.on('tick', function() end)\n"),
            ],
        );
        let report = check_project(&dir, false).unwrap();
        assert_eq!(codes(&report), ["script::tick-listener"]);
        assert!(report.passes(false));
        assert!(!report.passes(true));
        std::fs::remove_dir_all(dir).ok();
    }
}
