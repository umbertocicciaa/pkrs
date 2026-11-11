#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_core::{Severity, Version};
use pkrs_world::conventions::{ConventionViolation, ViolationKind, ViolationLocation};
use pkrs_world::raw::{RawLayer, RawMap, RawObject, RawOrientation};
use pkrs_world::{Properties, PropertyValue, build_map};

use super::*;
use crate::MemoryScripts;

const MANIFEST: &str = "[game]\nname = \"T\"\nid = \"com.example.t\"\n[engine]\nversion = \"0.0\"\napi = 1\n\
                        [start]\nmap = \"town\"\nposition = [0, 0]\n";

fn options() -> ValidateOptions<'static> {
    ValidateOptions {
        engine: Version::parse("0.0.0").unwrap(),
        supported_api: 1..=1,
        plugin_content: None,
    }
}

/// One test object: class, name and properties.
type TestObject<'a> = (&'a str, &'a str, &'a [(&'a str, PropertyValue)]);

/// A 4×4 map with `objects`.
fn map(id: &str, objects: &[TestObject<'_>]) -> Map {
    let objects = objects
        .iter()
        .enumerate()
        .map(|(i, (class, name, props))| RawObject {
            id: u32::try_from(i).unwrap() + 1,
            name: (*name).into(),
            class: (*class).into(),
            x: 0.0,
            y: 0.0,
            width: 16.0,
            height: 16.0,
            properties: props
                .iter()
                .fold(Properties::new(), |p, (k, v)| p.with(*k, v.clone())),
        })
        .collect();
    build_map(RawMap {
        id: id.into(),
        orientation: RawOrientation::Orthogonal,
        infinite: false,
        width: 4,
        height: 4,
        tile_width: 16,
        tile_height: 16,
        properties: Properties::new(),
        tilesets: Vec::new(),
        layers: vec![
            RawLayer::Tiles {
                name: "ground".into(),
                visible: true,
                tiles: vec![None; 16],
                properties: Properties::new(),
            },
            RawLayer::Objects {
                name: "objects".into(),
                objects,
            },
        ],
    })
    .unwrap()
}

fn s(v: &str) -> PropertyValue {
    PropertyValue::String(v.into())
}

fn run(files: &MemoryScripts, maps: Vec<Map>) -> ProjectValidation {
    let mut import = |path: &str| {
        maps.iter()
            .find(|m| format!("maps/{}.tmx", m.id) == path)
            .cloned()
            .ok_or_else(|| MapError::Conventions {
                path: path.into(),
                violations: vec![ConventionViolation {
                    location: ViolationLocation::Map,
                    kind: ViolationKind::MissingLayer("ground"),
                }],
            })
    };
    validate_project(files, &mut import, &options())
}

fn codes(v: &ProjectValidation) -> Vec<&str> {
    v.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn a_clean_project_has_no_diagnostics() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("maps/town.tmx", "")
        .with(
            "scripts/maps/town.lua",
            "function keeper(npc)\n  game.set_flag('met', true)\nend\n",
        );
    let town = map("town", &[("npc", "keeper", &[("script", s("keeper"))])]);
    let v = run(&files, vec![town]);
    assert!(v.diagnostics.is_empty(), "{}", v.diagnostics);
    assert!(v.manifest.is_some());
}

#[test]
fn manifest_errors_do_not_stop_the_other_checks() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST.replace("api = 1", "api = 7"))
        .with("scripts/main.lua", "function broken(\n");
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["manifest::invalid", "script::syntax"]);
    let syntax = &v.diagnostics.items()[1];
    assert_eq!(syntax.file.as_deref(), Some("scripts/main.lua"));
    assert!(syntax.span.is_some());
}

#[test]
fn deprecated_script_calls_warn() {
    let files = MemoryScripts::new().with("pkrs.toml", MANIFEST).with(
        "scripts/main.lua",
        "audio.play_jingle('ok')\naudio.jingle('fanfare')\n",
    );
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["script::deprecated"]);
    let warning = &v.diagnostics.items()[0];
    assert_eq!(warning.span.expect("span").line, 2);
    assert!(
        warning.message.contains("audio.play_jingle"),
        "{}",
        warning.message
    );
}

#[test]
fn missing_manifest_is_an_error() {
    let v = run(&MemoryScripts::new(), vec![]);
    assert_eq!(codes(&v), ["manifest::read"]);
    assert!(v.manifest.is_none());
}

#[test]
fn plugin_graph_errors_are_located_in_pkrs_toml() {
    let files = MemoryScripts::new().with(
        "pkrs.toml",
        format!("{MANIFEST}[plugins]\nenabled = [\"ghost\"]\n"),
    );
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["plugin::not-found"]);
    let d = &v.diagnostics.items()[0];
    assert_eq!(d.file.as_deref(), Some("pkrs.toml"));
    assert_eq!(d.span, Some(SourceSpan::new(11, 12, 7)));
}

#[test]
fn tick_listeners_are_warnings() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("scripts/main.lua", "events.on(\"tick\", function() end)\n");
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["script::tick-listener"]);
    assert_eq!(v.diagnostics.items()[0].severity, Severity::Warning);
    assert!(!v.diagnostics.has_errors());
}

#[test]
fn lua_text_literals_must_be_defined() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("data/text/en/a.ron", "{ \"known\": \"K\" }")
        .with(
            "scripts/main.lua",
            "ui.say(\"text:known\")\nui.say(\"text:unknown\")\n",
        );
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["script::unknown-text-key"]);
    assert_eq!(
        v.diagnostics.items()[0].span,
        Some(SourceSpan::new(2, 8, 14))
    );
}

#[test]
fn lua_text_literals_may_come_from_enabled_plugins() {
    let plugin = "[plugin]\nid = \"p\"\nname = \"p\"\nversion = \"1.0.0\"\napi = 1\n";
    let files = MemoryScripts::new()
        .with(
            "pkrs.toml",
            format!("{MANIFEST}[plugins]\nenabled = [\"p\"]\n"),
        )
        .with("plugins/p/plugin.toml", plugin)
        .with("plugins/p/data/text/en/p.ron", "{ \"p.hello\": \"Hi\" }")
        .with(
            "plugins/p/init.lua",
            "log.info(\"text:p.hello\")\nlog.info(\"text:p.missing\")\n",
        )
        .with("plugins/q/plugin.toml", plugin.replace("\"p\"", "\"q\""))
        .with("plugins/q/data/text/en/q.ron", "{ \"q.only\": \"Q\" }")
        .with(
            "scripts/main.lua",
            "ui.say(\"text:p.hello\")\nui.say(\"text:q.only\")\n",
        );
    let v = run(&files, vec![]);
    let unknown: Vec<_> = v
        .diagnostics
        .iter()
        .filter(|d| d.code == "script::unknown-text-key")
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(
        unknown,
        [
            "text key `p.missing` is not defined for the default language `en`",
            "text key `q.only` is not defined for the default language `en`",
        ],
        "only enabled plugins' tables count"
    );
}

#[test]
fn map_cross_references() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("maps/town.tmx", "")
        .with("maps/house.tmx", "")
        .with(
            "scripts/maps/town.lua",
            "function intro()\n  ui.say('hi')\nend\n",
        );
    let town = map(
        "town",
        &[
            (
                "warp",
                "",
                &[
                    ("to_map", s("cave")),
                    ("to_x", PropertyValue::Int(0)),
                    ("to_y", PropertyValue::Int(0)),
                ],
            ),
            (
                "warp",
                "",
                &[
                    ("to_map", s("house")),
                    ("to_x", PropertyValue::Int(9)),
                    ("to_y", PropertyValue::Int(0)),
                ],
            ),
            (
                "npc",
                "a",
                &[
                    ("script", s("maps/town.lua:missing")),
                    ("sprite", s("ghost")),
                ],
            ),
            ("npc", "b", &[("script", s("other.lua:x"))]),
            ("npc", "c", &[("script", s("cutscenes/intro"))]),
            ("sign", "", &[("text", s("text:nope"))]),
            ("item", "", &[("item", s("potion"))]),
            (
                "trigger",
                "t",
                &[
                    ("on_enter", s("intro")),
                    ("once", PropertyValue::Bool(true)),
                ],
            ),
        ],
    );
    let house = map("house", &[]);
    let v = run(&files, vec![house, town]);
    assert_eq!(
        codes(&v),
        [
            "map::unknown-map",
            "map::warp-out-of-bounds",
            "map::unknown-script-function",
            "map::missing-sprite",
            "map::unknown-script",
            "map::invalid-script-ref",
            "text::missing-key",
            "map::unknown-item",
            "map::once-trigger-without-state",
        ]
    );
    let once = v.diagnostics.iter().last().unwrap();
    assert_eq!(once.severity, Severity::Warning);
    assert_eq!(once.file.as_deref(), Some("maps/town.tmx"));
}

#[test]
fn convention_errors_of_every_map_are_reported() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("maps/a.tmx", "<map>")
        .with("maps/b.tmx", "<map>");
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["map::convention", "map::convention"]);
}

#[test]
fn ids_registered_by_plugin_scripts_are_known() {
    let plugin = "[plugin]\nid = \"p\"\nname = \"p\"\nversion = \"1.0.0\"\napi = 1\n";
    let files = MemoryScripts::new()
        .with("pkrs.toml", format!("{MANIFEST}[plugins]\nenabled = [\"p\"]\n"))
        .with("plugins/p/plugin.toml", plugin)
        .with("plugins/p/init.lua", "data.register_move({ id = \"zap\", type = \"normal\" })\n")
        .with(
            "data/species/a.ron",
            "(id: \"a\", types: [\"normal\"], hp: 1, attack: 1, defense: 1, speed: 1, learnset: [(level: 1, move: \"zap\"), (level: 2, move: \"nope\")])",
        );
    let v = run(&files, vec![]);
    assert_eq!(codes(&v), ["data::unknown-move"]);
    assert!(v.diagnostics.items()[0].message.contains("`nope`"));
    assert_eq!(v.plan.unwrap().ids(), ["p"]);
}

#[test]
fn every_problem_of_a_broken_project_is_reported_in_one_run() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST.replace("com.example.t", "Bad Id"))
        .with("data/moves/bad.ron", "(id: \"x\",")
        .with(
            "data/species/a.ron",
            "(id: \"a\", types: [\"ghost\"], hp: 1, attack: 1, defense: 1, speed: 1)",
        )
        .with("scripts/main.lua", "if then end")
        .with("maps/town.tmx", "");
    let v = run(&files, vec![]);
    assert_eq!(
        codes(&v),
        [
            "manifest::invalid",
            "data::parse",
            "script::syntax",
            "map::convention"
        ],
        "{}",
        v.diagnostics
    );
}

#[test]
fn materials_are_checked_and_map_references_resolve() {
    let files = MemoryScripts::new()
        .with("pkrs.toml", MANIFEST)
        .with("maps/town.tmx", "")
        .with(
            "assets/materials/water.ron",
            "(shader: \"pkrs/animated_tiles\", uniforms: { \"frames\": Int(2), \"ticks_per_frame\": Int(8), \"frame_offset\": Vec2(16.0, 0.0) })",
        )
        .with(
            "assets/materials/fx/glow.ron",
            "(shader: \"shaders/glow.glsl\", profile: (glsl: \"100\", max_uniforms: 1, texture_slots: 4))",
        )
        .with(
            "assets/shaders/glow.glsl",
            "#version 100\nuniform sampler2D Texture;\nuniform vec4 glow;\nuniform float pulse;\nvoid main() {}\n",
        );
    let mut town = map("town", &[("npc", "ghost", &[("material", s("ghostly"))])]);
    town.layers[0].material = Some("water".into());
    let v = run(&files, vec![town]);
    assert_eq!(
        codes(&v),
        [
            "material::unsupported-profile",
            "material::too-many-uniforms",
            "material::unset-uniform",
            "material::unset-uniform",
            "material::unknown-material",
        ],
        "{}",
        v.diagnostics
    );
    let profile = &v.diagnostics.items()[0];
    assert_eq!(
        profile.file.as_deref(),
        Some("assets/materials/fx/glow.ron")
    );
    assert!(profile.span.is_some());
    let unknown = &v.diagnostics.items()[4];
    assert_eq!(unknown.file.as_deref(), Some("maps/town.tmx"));
    assert!(
        unknown.message.contains("assets/materials/ghostly.ron"),
        "{}",
        unknown.message
    );
}
