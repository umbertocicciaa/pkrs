#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_core::{Diagnostic, Severity, SourceSpan};

use super::*;
use crate::database::{DATA_ROOT, GameDataBuilder, LoadOptions};
use crate::files::MemoryDataFiles;

const TYPES: &str = r#"TypeChart(types: ["normal", "fire"], chart: {})"#;
const MOVES: &str = r#"[
  (id: "scratch", type: "normal", category: physical, power: 40, accuracy: 100, priority: 0,
   effects: [(effect: "damage")]),
]"#;

fn base() -> MemoryDataFiles {
    MemoryDataFiles::new()
        .with("data/types.ron", TYPES)
        .with("data/moves/moves.ron", MOVES)
        .with("data/text/en/a.ron", r#"{ "species.a": "A" }"#)
        .with("data/text/it/a.ron", r#"{ "other": "B" }"#)
}

fn check(files: &MemoryDataFiles) -> Vec<Diagnostic> {
    check_with(files, &|_| false)
}

fn check_with(files: &MemoryDataFiles, registered: &dyn Fn(&str) -> bool) -> Vec<Diagnostic> {
    let mut builder = GameDataBuilder::new(LoadOptions::default());
    builder.load_dir(files, DATA_ROOT);
    let (data, errors) = builder.finish();
    assert!(errors.is_empty(), "{errors:?}");
    let available = vec!["en".to_owned(), "it".to_owned()];
    let ctx = RefContext {
        files,
        locale: TextLocale {
            default: "en",
            available: &available,
        },
        registered_elsewhere: registered,
    };
    check_game_data(&data, &ctx)
}

fn codes(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn species(body: &str) -> MemoryDataFiles {
    base().with(
        "data/species/a.ron",
        format!("Species(\n  id: \"a\",\n  {body}\n)"),
    )
}

#[test]
fn clean_content_has_no_diagnostics() {
    let files = species(r#"name: "text:species.a", types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1, learnset: [(level: 1, move: "scratch")]"#)
        .with("data/text/it/b.ron", r#"{ "species.a": "A" }"#);
    assert_eq!(check(&files), []);
}

#[test]
fn unknown_type_is_reported_at_the_value() {
    let files = species(r#"types: ["water"], hp: 1, attack: 1, defense: 1, speed: 1"#);
    let diags = check(&files);
    assert_eq!(codes(&diags), ["data::unknown-type"]);
    assert_eq!(diags[0].file.as_deref(), Some("data/species/a.ron"));
    assert_eq!(diags[0].span, Some(SourceSpan::new(3, 11, 7)));
    assert_eq!(
        diags[0].message,
        "species `a` references unknown type `water`"
    );
}

#[test]
fn unknown_moves_abilities_and_evolutions() {
    let files = species(
        r#"types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1, abilities: ["kindle"],
  learnset: [(level: 1, move: "vine")], evolutions: [(into: "b", trigger: Level(5))]"#,
    );
    assert_eq!(
        codes(&check(&files)),
        [
            "data::unknown-ability",
            "data::unknown-move",
            "data::unknown-species"
        ]
    );
}

#[test]
fn ids_registered_by_plugin_scripts_are_not_unknown() {
    let files = species(
        r#"types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1, learnset: [(level: 1, move: "zap")]"#,
    );
    assert_eq!(check_with(&files, &|id| id == "zap"), []);
}

#[test]
fn missing_sprites_are_errors_and_assets_folder_counts() {
    let files = species(
        r#"types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1,
  sprites: (front: "sprites/a.png", back: "sprites/a_back.png", icon: "p:icons/a.png")"#,
    )
    .with("assets/sprites/a.png", "")
    .with("plugins/p/assets/icons/a.png", "");
    let diags = check(&files);
    assert_eq!(codes(&diags), ["data::missing-sprite"]);
    assert!(diags[0].message.contains("sprites/a_back.png"));
}

#[test]
fn text_keys_must_exist_in_the_default_language() {
    let files = species(
        r#"name: "text:species.nope", types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1"#,
    );
    let diags = check(&files);
    assert_eq!(codes(&diags), ["text::missing-key"]);
    assert_eq!(diags[0].severity, Severity::Error);
    assert_eq!(diags[0].span.map(|s| s.line), Some(3));
}

#[test]
fn untranslated_keys_are_warnings() {
    let files = species(
        r#"name: "text:species.a", types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1"#,
    );
    let diags = check(&files);
    assert_eq!(codes(&diags), ["text::missing-translation"]);
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0].message.contains("`it`"));
}

#[test]
fn text_refs_reject_malformed_keys() {
    let text = crate::text::TextDb::new();
    let locale = TextLocale {
        default: "en",
        available: &[],
    };
    assert_eq!(
        codes(&check_text_ref(&text, &locale, "text:Bad Key")),
        ["text::invalid-key"]
    );
    assert!(check_text_ref(&text, &locale, "Literal text").is_empty());
}

#[test]
fn species_schema_rules() {
    let files =
        species(r#"types: ["fire", "fire", "normal"], hp: 0, attack: 1, defense: 1, speed: 1"#);
    let diags = check(&files);
    let messages: Vec<&str> = diags.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "species `a` has 3 types; at most 2 are allowed",
            "species `a` lists type `fire` twice",
            "species `a` has 0 base HP",
        ]
    );
    let files = species("types: [], hp: 1, attack: 1, defense: 1, speed: 1");
    assert_eq!(check(&files)[0].message, "species `a` has no type");
}

#[test]
fn move_schema_and_status_rules() {
    let files = base().with(
        "data/moves/bad.ron",
        r#"(id: "bad", type: "ghost", category: status, power: 0, accuracy: 100, priority: 9, pp: 0,
   effects: [(effect: "apply_status", status: "doom", chance: 10), (effect: "stat_stage", stat: "attack", stages: 0)])"#,
    );
    let diags = check(&files);
    assert_eq!(
        codes(&diags),
        [
            "data::schema",
            "data::schema",
            "data::schema",
            "data::unknown-type",
            "data::unknown-status"
        ]
    );
    // Field rules point at the field name.
    assert_eq!(diags[0].span, Some(SourceSpan::new(1, 84, 2)));
}

#[test]
fn trainer_references() {
    let files = base().with(
        "data/trainers/t.ron",
        r#"Trainer(id: "t", party: [(species: "ghost", level: 5, moves: ["nope"])], items: ["potion"], ai_profile: "profiles/boss")"#,
    );
    assert_eq!(
        codes(&check(&files)),
        [
            "data::unknown-species",
            "data::unknown-move",
            "data::unknown-item",
            "data::unknown-ai-profile"
        ]
    );
    let builtin = base().with(
        "data/trainers/t.ron",
        r#"Trainer(id: "t", party: [(species: "ghost", level: 5)], ai_profile: "smart")"#,
    );
    assert_eq!(codes(&check(&builtin)), ["data::unknown-species"]);
}

#[test]
fn encounter_and_ai_script_references() {
    let files = base()
        .with(
            "data/encounters/e.ron",
            r#"(id: "e", slots: [(species: "ghost", min_level: 1, max_level: 2, weight: 1)])"#,
        )
        .with(
            "data/trainers/profiles/p.ron",
            r#"AiProfile(id: "p", tier: Smart, script: "scripts/ai/p.lua")"#,
        );
    assert_eq!(
        codes(&check(&files)),
        ["data::missing-script", "data::unknown-species"]
    );
}

#[test]
fn load_errors_carry_file_and_position() {
    let files = base()
        .with("data/moves/broken.ron", "(id: \"x\",\n  type: )")
        .with(
            "data/species/a.ron",
            r#"(id: "a", types: ["fire"], hp: 1, attack: 1, defense: 1, speed: 1)"#,
        )
        .with(
            "data/species/b.ron",
            "\n(id: \"a\", types: [\"fire\"], hp: 1, attack: 1, defense: 1, speed: 1)",
        );
    let mut builder = GameDataBuilder::new(LoadOptions::default());
    builder.load_dir(&files, DATA_ROOT);
    let (_, errors) = builder.finish();
    let diags: Vec<Diagnostic> = errors
        .iter()
        .map(|e| {
            let src =
                data_error_file(e).and_then(|f| crate::files::DataFiles::read_text(&files, f).ok());
            data_error_diagnostic(e, src.as_deref())
        })
        .collect();
    assert_eq!(codes(&diags), ["data::duplicate-id", "data::parse"]);
    assert_eq!(diags[0].file.as_deref(), Some("data/species/b.ron"));
    assert_eq!(diags[0].span, Some(SourceSpan::new(2, 6, 3)));
    assert_eq!(diags[1].file.as_deref(), Some("data/moves/broken.ron"));
    assert_eq!(diags[1].span.map(|s| s.line), Some(2));
}

#[test]
fn schema_load_errors_point_at_the_field() {
    let files = MemoryDataFiles::new().with(
        "data/species/a.ron",
        "(id: \"a\", types: [\"fire\"], hp: 1, attack: 1, defense: 1, speed: 1,\n learnset: [(level: 0, move: \"x\")])",
    );
    let mut builder = GameDataBuilder::new(LoadOptions::default());
    builder.load_dir(&files, DATA_ROOT);
    let (_, errors) = builder.finish();
    let src = crate::files::DataFiles::read_text(&files, "data/species/a.ron").unwrap();
    let d = data_error_diagnostic(&errors[0], Some(&src));
    assert_eq!(d.code, "data::schema");
    assert_eq!(d.span, Some(SourceSpan::new(2, 2, 8)));
}
