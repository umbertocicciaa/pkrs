//! Map cross-references (§6.2, §6.4, §6.5): warps, object scripts, sprites, items,
//! encounter tables, sign text and the `once`-trigger heuristic.

use std::collections::BTreeMap;

use pkrs_core::{Diagnostic, Diagnostics, SourceSpan};
use pkrs_data::GameData;
use pkrs_data::validate::{TextLocale, check_text_ref};
use pkrs_world::diagnostics::{map_error_diagnostics, object_span, property_span};
use pkrs_world::{Map, MapError, MapObject, ObjectKind};

use super::lua_scan::LuaFacts;
use super::{MAPS_DIR, SCRIPTS_DIR};
use crate::host::ScriptFiles;

/// Game script that runs at start-up; bare object script names fall back to it (§9.2).
const MAIN_SCRIPT: &str = "scripts/main.lua";

/// Imports every `maps/*.tmx` (in name order); failures become diagnostics.
pub(super) fn import_all(
    files: &dyn ScriptFiles,
    import_map: &mut dyn FnMut(&str) -> Result<Map, MapError>,
    out: &mut Diagnostics,
) -> Vec<(String, Map)> {
    let mut names: Vec<String> = files
        .list_files(MAPS_DIR)
        .into_iter()
        .filter(|n| n.ends_with(".tmx"))
        .collect();
    names.sort();
    let mut maps = Vec::new();
    for name in names {
        let path = format!("{MAPS_DIR}/{name}");
        match import_map(&path) {
            Ok(map) => maps.push((path, map)),
            Err(err) => {
                let source = files.read_script(&path).ok();
                out.extend(map_error_diagnostics(&err, &path, source.as_deref()));
            }
        }
    }
    maps
}

/// A script reference resolved to its file and function.
struct ScriptTarget<'a> {
    file: String,
    function: &'a str,
}

pub(super) struct MapCheck<'a> {
    pub files: &'a dyn ScriptFiles,
    pub data: &'a GameData,
    pub locale: TextLocale<'a>,
    pub lua: &'a BTreeMap<String, LuaFacts>,
}

/// The checks of one map, with its source for spans.
struct Checker<'a, 'b> {
    ctx: &'b MapCheck<'a>,
    path: &'b str,
    map: &'b Map,
    source: Option<String>,
    out: &'b mut Diagnostics,
}

impl MapCheck<'_> {
    /// Checks `map` (from `path`); `all` are every imported map (warp targets).
    pub(super) fn check(
        &self,
        path: &str,
        map: &Map,
        all: &[(String, Map)],
        out: &mut Diagnostics,
    ) {
        let mut c = Checker {
            ctx: self,
            path,
            map,
            source: self.files.read_script(path).ok(),
            out,
        };
        for tileset in &map.tilesets {
            let Some(image) = &tileset.image else {
                continue;
            };
            if self.files.exists(&image.path) {
                continue;
            }
            let span = c
                .source
                .as_deref()
                .and_then(|src| SourceSpan::find(src, "<tileset", 0));
            c.out.push(
                Diagnostic::error(
                    "map::missing-tileset-image",
                    format!(
                        "tileset `{}` uses image `{}`, which does not exist",
                        tileset.name, image.path
                    ),
                )
                .in_file(path)
                .at(span)
                .with_label("image not found"),
            );
        }
        for object in &map.objects {
            c.object(object, all);
        }
        if self.data.battles_enabled() {
            for zone in &map.encounters {
                if self.data.encounters().contains(&zone.table) {
                    continue;
                }
                let span = c.property(zone.id, "table");
                c.out.push(
                    Diagnostic::error(
                        "map::unknown-encounter-table",
                        format!(
                            "encounter rect #{} uses unknown encounter table `{}`",
                            zone.id, zone.table
                        ),
                    )
                    .in_file(path)
                    .at(span)
                    .with_label("no encounter table with this id")
                    .with_help("define it in data/encounters/"),
                );
            }
        }
    }
}

impl Checker<'_, '_> {
    fn property(&self, id: u32, key: &str) -> Option<SourceSpan> {
        self.source
            .as_deref()
            .and_then(|src| property_span(src, id, key))
    }

    fn report(&mut self, object: &MapObject, key: Option<&str>, diagnostic: Diagnostic) {
        let span = match key {
            Some(key) => self.property(object.id, key),
            None => self
                .source
                .as_deref()
                .and_then(|src| object_span(src, object.id)),
        };
        self.out.push(diagnostic.in_file(self.path).at(span));
    }

    fn object(&mut self, object: &MapObject, all: &[(String, Map)]) {
        let who = describe(object);
        match &object.kind {
            ObjectKind::Warp(warp) => match all.iter().find(|(_, m)| m.id == warp.to_map) {
                None => self.report(
                    object,
                    Some("to_map"),
                    Diagnostic::error(
                        "map::unknown-map",
                        format!("{who} leads to unknown map `{}`", warp.to_map),
                    )
                    .with_label("no such map")
                    .with_help(format!(
                        "maps are the files in {MAPS_DIR}/ (`{MAPS_DIR}/<id>.tmx`)"
                    )),
                ),
                Some((_, target)) => {
                    let (x, y) = (warp.to.x, warp.to.y);
                    let inside =
                        x >= 0 && y >= 0 && (x as u32) < target.width && (y as u32) < target.height;
                    if !inside {
                        self.report(
                            object,
                            Some("to_x"),
                            Diagnostic::error(
                                "map::warp-out-of-bounds",
                                format!(
                                    "{who} leads to tile ({x}, {y}), outside map `{}` ({}x{})",
                                    target.id, target.width, target.height
                                ),
                            )
                            .with_label("outside the target map"),
                        );
                    }
                }
            },
            ObjectKind::Npc { script, sprite, .. } => {
                if let Some(script) = script {
                    self.script_ref(object, "script", script);
                }
                if let Some(sprite) = sprite {
                    let path = format!("sprites/{sprite}.png");
                    if !pkrs_data::validate::asset_exists(&super::DataView(self.ctx.files), &path) {
                        self.report(
                            object,
                            Some("sprite"),
                            Diagnostic::error(
                                "map::missing-sprite",
                                format!(
                                    "{who} uses sprite `{sprite}`, but `{path}` does not exist"
                                ),
                            )
                            .with_label("sprite not found")
                            .with_help("NPC sprites are `sprites/<sprite>.png`"),
                        );
                    }
                }
            }
            ObjectKind::Trigger { on_enter, once } => {
                let Some(on_enter) = on_enter else {
                    return;
                };
                let target = self.script_ref(object, "on_enter", on_enter);
                if let (true, Some(target)) = (*once, target) {
                    let sets = self
                        .ctx
                        .lua
                        .get(&target.file)
                        .is_some_and(|facts| facts.sets_state(target.function));
                    if !sets {
                        self.report(
                            object,
                            Some("once"),
                            Diagnostic::warning(
                                "map::once-trigger-without-state",
                                format!(
                                    "{who} is `once`, but `{}` in `{}` never sets a flag or variable",
                                    target.function, target.file
                                ),
                            )
                            .with_label("fires once per save")
                            .with_help("record the story progress with `game.set_flag`, `game.set_var` or `map.set_var` (§6.5)"),
                        );
                    }
                }
            }
            ObjectKind::Sign { script, text } => {
                if let Some(script) = script {
                    self.script_ref(object, "script", script);
                }
                if let Some(text) = text {
                    for d in check_text_ref(self.ctx.data.text(), &self.ctx.locale, text) {
                        self.report(object, Some("text"), d);
                    }
                }
            }
            ObjectKind::Item { item, .. } => {
                if !self.ctx.data.items().contains(item) {
                    self.report(
                        object,
                        Some("item"),
                        Diagnostic::error(
                            "map::unknown-item",
                            format!("{who} gives unknown item `{item}`"),
                        )
                        .with_label("no item with this id")
                        .with_help("define it in data/items/"),
                    );
                }
            }
            ObjectKind::Spawn { .. } => {}
        }
    }

    /// Resolves an object script reference like the runtime (§9.2): `file.lua:function`
    /// below `scripts/`, or a bare `function` of the map script, else of `scripts/main.lua`.
    /// Reports what does not resolve.
    fn script_ref<'s>(
        &mut self,
        object: &MapObject,
        key: &str,
        reference: &'s str,
    ) -> Option<ScriptTarget<'s>> {
        let who = describe(object);
        let lua = self.ctx.lua;
        if let Some((file, function)) = reference.rsplit_once(':') {
            let file = format!("{SCRIPTS_DIR}/{}", file.trim_start_matches('/'));
            let Some(facts) = lua.get(&file) else {
                self.report(
                    object,
                    Some(key),
                    Diagnostic::error(
                        "map::unknown-script",
                        format!(
                            "{who} references script `{reference}`, but `{file}` does not exist"
                        ),
                    )
                    .with_label("script file not found"),
                );
                return None;
            };
            if !facts.functions.contains_key(function) {
                self.report(
                    object,
                    Some(key),
                    Diagnostic::error(
                        "map::unknown-script-function",
                        format!("{who} references `{reference}`, but `{file}` defines no global function `{function}`"),
                    )
                    .with_label("function not found")
                    .with_help(format!("define `function {function}(…)` in {file}")),
                );
                return None;
            }
            return Some(ScriptTarget { file, function });
        }
        if reference.contains('/') || reference.is_empty() {
            self.report(
                object,
                Some(key),
                Diagnostic::error("map::invalid-script-ref", format!("{who} has script reference `{reference}`"))
                    .with_label("not a script reference")
                    .with_help("write `file.lua:function` (relative to scripts/) or a function of the map script"),
            );
            return None;
        }
        let map_script = format!("{SCRIPTS_DIR}/maps/{}.lua", self.map.id);
        for file in [map_script.as_str(), MAIN_SCRIPT] {
            if lua
                .get(file)
                .is_some_and(|f| f.functions.contains_key(reference))
            {
                return Some(ScriptTarget {
                    file: file.to_owned(),
                    function: reference,
                });
            }
        }
        self.report(
            object,
            Some(key),
            Diagnostic::error(
                "map::unknown-script-function",
                format!("{who} references function `{reference}`, which neither `{map_script}` nor `{MAIN_SCRIPT}` defines"),
            )
            .with_label("function not found")
            .with_help(format!("define `function {reference}(…)` in {map_script}")),
        );
        None
    }
}

fn describe(object: &MapObject) -> String {
    let class = object.kind.class_name();
    if object.name.is_empty() {
        format!("{class} #{}", object.id)
    } else {
        format!("{class} `{}`", object.name)
    }
}
