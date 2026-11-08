//! Project validation (§8.2, §2.3 `pkrs check`): every check that does not need a running
//! game, shared by `pkrs check` and the runtime's start-up check. Problems are collected as
//! vendor-free [`Diagnostic`]s with file, line and column; nothing is fail-fast.
//!
//! [`validate_project`] runs, in order:
//! 1. **Manifest** — `pkrs.toml` (parse, validation, engine version and API level). When it
//!    is invalid the other checks still run, with default settings.
//! 2. **Plugin graph** — `plugins/*/plugin.toml` of the enabled plugins, missing plugins,
//!    dependencies, versions, conflicts and cycles (§9.4), and the plugins' `data/` folders.
//! 3. **Content** — the game's `data/` folder: parse (stage 1), schema (stage 2) and
//!    cross-references (stage 3) through [`pkrs_data::validate`].
//! 4. **Lua** — every `.lua` file below `scripts/` and `plugins/` is compiled (never run);
//!    `events.on("tick", …)` is a performance warning, `"text:…"` literals must be defined.
//! 5. **Maps** — every `maps/*.tmx` is imported (§6.2 conventions); warps, NPC / trigger /
//!    sign scripts, sprites, items, encounter tables and sign text are cross-checked, and a
//!    `once` trigger whose script never sets a flag or variable is a warning (§6.5);
//!    `maps/world.ron` names existing maps.
//!
//! The rule codes (`data::unknown-move`, `map::once-trigger-without-state`, …) are stable so
//! CI logs can be searched for them.

mod lua_scan;
mod maps;
mod plugins;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use pkrs_core::{Diagnostic, Diagnostics, ProjectManifest, Severity, SourceSpan, Version};
use pkrs_data::validate::{
    RefContext, TextLocale, check_game_data, check_text_ref, data_error_diagnostic, data_error_file,
};
use pkrs_data::{ContentDb, DATA_ROOT, DataFiles, GameData, GameDataBuilder, LoadOptions};
use pkrs_world::{Map, MapError, WORLD_RON, WorldLayout};

pub use lua_scan::{LuaFacts, LuaFunction, STATE_SETTERS};
pub use plugins::{plugin_error_diagnostics, plugin_init_diagnostic};

use crate::check_lua_syntax;
use crate::host::ScriptFiles;
use crate::plugin::{ContentError, PLUGINS_DIR, PluginPlan, merge_content, plan_plugins};

/// Folder of game scripts.
pub const SCRIPTS_DIR: &str = "scripts";
/// Folder of maps.
pub const MAPS_DIR: &str = "maps";

/// What [`validate_project`] checks against.
#[derive(Clone, Debug)]
pub struct ValidateOptions<'a> {
    /// The engine version `engine.version` must accept.
    pub engine: Version,
    /// Scripting API levels the engine supports (§9.7).
    pub supported_api: RangeInclusive<u32>,
    /// The content database after the plugins' `init.lua` ran (`pkrs check --plugins`, the
    /// runtime). Without it, ids that plugin scripts may register with `data.register_*`
    /// (string literals of those scripts) are not reported as unknown.
    pub plugin_content: Option<&'a ContentDb>,
}

/// What [`validate_project`] found.
#[derive(Debug, Default)]
pub struct ProjectValidation {
    /// Every problem, in check order.
    pub diagnostics: Diagnostics,
    /// The manifest, when it is valid.
    pub manifest: Option<ProjectManifest>,
    /// The resolved plugin set, when it is valid.
    pub plan: Option<PluginPlan>,
}

/// Runs every check of the module docs on the project in `files`. `import_map` imports one
/// map file (`maps/town.tmx`) into the engine model (the Tiled adapter in practice).
pub fn validate_project(
    files: &dyn ScriptFiles,
    import_map: &mut dyn FnMut(&str) -> Result<Map, MapError>,
    options: &ValidateOptions<'_>,
) -> ProjectValidation {
    let mut out = ProjectValidation::default();
    let data_files = DataView(files);

    // 1. Manifest.
    let manifest = check_manifest(files, options, &mut out.diagnostics);
    let features = manifest.as_ref().map(|m| m.features).unwrap_or_default();
    let (default_lang, available) = match &manifest {
        Some(m) => (m.locale.default.clone(), m.locale.available.clone()),
        None => ("en".to_owned(), vec!["en".to_owned()]),
    };
    let locale = TextLocale {
        default: &default_lang,
        available: &available,
    };

    // 2. Plugin graph and plugin content.
    let enabled = manifest
        .as_ref()
        .map(|m| m.plugins.enabled.clone())
        .unwrap_or_default();
    let plan = match plan_plugins(files, &enabled, options.supported_api.clone()) {
        Ok(plan) => Some(plan),
        Err(errors) => {
            for err in errors.errors() {
                out.diagnostics.extend(plugin_error_diagnostics(err, files));
            }
            None
        }
    };
    let plugin_ids: Vec<&str> = plan.as_ref().map(PluginPlan::ids).unwrap_or_default();
    let merged = merge_content(files, &plugin_ids);
    for err in merged
        .errors
        .iter()
        .filter(|e| e.file.starts_with(PLUGINS_DIR))
    {
        out.diagnostics.push(content_error_diagnostic(err, files));
    }

    // 3. Content.
    let mut builder = GameDataBuilder::new(LoadOptions::from_features(&features));
    builder.load_dir(&data_files, DATA_ROOT);
    builder.merge_content(options.plugin_content.unwrap_or(&merged.db));
    let (data, errors) = builder.finish();
    for err in &errors {
        let source = data_error_file(err).and_then(|f| files.read_script(f).ok());
        out.diagnostics
            .push(data_error_diagnostic(err, source.as_deref()));
    }

    // 4. Lua.
    let mut lua = BTreeMap::new();
    for path in lua_files(files) {
        let Ok(source) = files.read_script(&path) else {
            continue;
        };
        let facts = LuaFacts::scan(&source);
        check_lua_file(&path, &source, &facts, &data, &locale, &mut out.diagnostics);
        lua.insert(path, facts);
    }
    let registered: BTreeSet<String> = if options.plugin_content.is_some() {
        BTreeSet::new()
    } else {
        lua.iter()
            .filter(|(path, facts)| path.starts_with(PLUGINS_DIR) && facts.registers_content)
            .flat_map(|(_, facts)| facts.strings.iter().map(|(s, _)| s.clone()))
            .collect()
    };

    // Content cross-references (needs the Lua facts for plugin-registered ids).
    let is_registered = |id: &str| registered.contains(id);
    let ctx = RefContext {
        files: &data_files,
        locale,
        registered_elsewhere: &is_registered,
    };
    out.diagnostics.extend(check_game_data(&data, &ctx));

    // 5. Maps.
    let maps = maps::MapCheck {
        files,
        data: &data,
        locale,
        lua: &lua,
    };
    let imported = maps::import_all(files, import_map, &mut out.diagnostics);
    for (path, map) in &imported {
        maps.check(path, map, &imported, &mut out.diagnostics);
    }
    check_world_layout(files, &imported, &mut out.diagnostics);

    out.diagnostics.dedup();
    out.manifest = manifest;
    out.plan = plan;
    out
}

fn check_manifest(
    files: &dyn ScriptFiles,
    options: &ValidateOptions<'_>,
    out: &mut Diagnostics,
) -> Option<ProjectManifest> {
    let file = ProjectManifest::FILE_NAME;
    let source = match files.read_script(file) {
        Ok(source) => source,
        Err(err) => {
            out.push(
                Diagnostic::error("manifest::read", format!("cannot read {file}: {err}"))
                    .in_file(file),
            );
            return None;
        }
    };
    match ProjectManifest::from_toml_str(&source) {
        Ok(manifest) => {
            if let Err(err) = manifest.check_engine(&options.engine, options.supported_api.clone())
            {
                out.extend(err.diagnostics(file, Some(&source)));
            }
            Some(manifest)
        }
        Err(err) => {
            out.extend(err.diagnostics(file, Some(&source)));
            None
        }
    }
}

fn content_error_diagnostic(err: &ContentError, files: &dyn ScriptFiles) -> Diagnostic {
    let source = files.read_script(&err.file).ok();
    let mut d = data_error_diagnostic(&err.source, source.as_deref());
    d.file = Some(err.file.clone());
    d
}

/// Compiles one Lua file and reports tick listeners and undefined `"text:…"` literals.
fn check_lua_file(
    path: &str,
    source: &str,
    facts: &LuaFacts,
    data: &GameData,
    locale: &TextLocale<'_>,
    out: &mut Diagnostics,
) {
    if let Err(err) = check_lua_syntax(path, source) {
        let span = err.line.map(|line| line_span(source, line as usize));
        out.push(
            Diagnostic::error(
                "script::syntax",
                format!("Lua syntax error: {}", err.message),
            )
            .in_file(path)
            .at(span)
            .with_label(err.message.clone()),
        );
    }
    for range in &facts.tick_listeners {
        out.push(
            Diagnostic::warning("script::tick-listener", "`events.on(\"tick\")` runs Lua every tick")
                .in_file(path)
                .at(Some(SourceSpan::from_byte_range(source, range.clone())))
                .with_label("runs 60 times per second")
                .with_help("prefer `step`, `map_enter` or `time.wait` in a task; tick listeners cost frame time (§9.8)"),
        );
    }
    for (value, range) in facts.strings_with_prefix(pkrs_data::TEXT_PREFIX) {
        for d in check_text_ref(data.text(), locale, value) {
            // A literal may not be shown as text, so a Lua literal is only a warning.
            let d = if d.code == "text::missing-key" {
                Diagnostic {
                    severity: Severity::Warning,
                    code: "script::unknown-text-key".into(),
                    ..d
                }
            } else {
                d
            };
            out.push(
                d.in_file(path)
                    .at(Some(SourceSpan::from_byte_range(source, range.clone()))),
            );
        }
    }
}

/// The span of line `line`'s text (without leading whitespace).
fn line_span(source: &str, line: usize) -> SourceSpan {
    let text = source
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default();
    let lead = text.chars().take_while(|c| c.is_whitespace()).count();
    SourceSpan::new(line, lead + 1, text.trim().chars().count())
}

/// Every `.lua` file below `scripts/` and `plugins/`, sorted.
fn lua_files(files: &dyn ScriptFiles) -> Vec<String> {
    let mut out = Vec::new();
    for root in [SCRIPTS_DIR, PLUGINS_DIR] {
        walk(files, root, 0, &mut out);
    }
    out.sort();
    out
}

fn walk(files: &dyn ScriptFiles, dir: &str, depth: usize, out: &mut Vec<String>) {
    if depth > 16 {
        return;
    }
    for name in files.list_files(dir) {
        if name.ends_with(".lua") {
            out.push(format!("{dir}/{name}"));
        }
    }
    for sub in files.list_dirs(dir) {
        // Plugin assets and data hold no scripts.
        if depth == 1 && dir.starts_with(PLUGINS_DIR) && (sub == "assets" || sub == "data") {
            continue;
        }
        walk(files, &format!("{dir}/{sub}"), depth + 1, out);
    }
}

/// `maps/world.ron` parses and names existing maps.
fn check_world_layout(files: &dyn ScriptFiles, maps: &[(String, Map)], out: &mut Diagnostics) {
    if !files.exists(WORLD_RON) {
        return;
    }
    let Ok(source) = files.read_script(WORLD_RON) else {
        return;
    };
    let layout = match WorldLayout::parse(&source, WORLD_RON) {
        Ok(layout) => layout,
        Err(err) => {
            out.push(Diagnostic::error("map::world-layout", err.to_string()).in_file(WORLD_RON));
            return;
        }
    };
    let sizes: BTreeMap<String, (u32, u32)> = maps
        .iter()
        .map(|(_, m)| (m.id.clone(), (m.width, m.height)))
        .collect();
    for id in layout.map_ids() {
        for name in std::iter::once(id).chain(layout.neighbor_ids(id)) {
            if !sizes.contains_key(name) {
                let span = SourceSpan::find(&source, &format!("\"{name}\""), 0);
                out.push(
                    Diagnostic::error(
                        "map::unknown-map",
                        format!(
                            "`{WORLD_RON}` names map `{name}`, but `maps/{name}.tmx` does not exist"
                        ),
                    )
                    .in_file(WORLD_RON)
                    .at(span)
                    .with_label("no such map"),
                );
            }
        }
    }
    if let Err(err) = layout.validate_layout(WORLD_RON, &sizes) {
        out.push(Diagnostic::error("map::world-layout", err.to_string()).in_file(WORLD_RON));
    }
}

/// [`DataFiles`] over a [`ScriptFiles`].
struct DataView<'a>(&'a dyn ScriptFiles);

impl DataFiles for DataView<'_> {
    fn list_files(&self, dir: &str) -> Vec<String> {
        self.0.list_files(dir)
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        self.0.list_dirs(dir)
    }

    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        self.0.read_script(path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
