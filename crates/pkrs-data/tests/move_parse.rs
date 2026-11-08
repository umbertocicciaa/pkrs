//! Move and effect RON parsing.

use pkrs_data::{BattlePack, DataError, MoveCategory, MoveDef, MoveEffect, TypeChart};

#[test]
fn parse_move_with_effects() {
    let mv = MoveDef::parse_ron(include_str!("fixtures/growth.ron"), "growth.ron").expect("parse");
    assert_eq!(mv.id, "growth");
    assert_eq!(mv.category, MoveCategory::Status);
    assert!(matches!(
        mv.effects[0],
        MoveEffect::StatStage { stages: 1, .. }
    ));
}

#[test]
fn design_doc_move_parses() {
    let mv = MoveDef::parse_ron(
        r#"Move(
          id: "ember", name: "text:moves.ember", type: "fire", category: Special,
          power: 40, accuracy: 100, pp: 25, priority: 0, target: SingleOpponent,
          effects: [ (effect: "damage"), (effect: "apply_status", status: "burn", chance: 10) ],
        )"#,
        "ember.ron",
    )
    .expect("parse");
    assert_eq!(mv.name, "text:moves.ember");
    assert_eq!((mv.category, mv.pp), (MoveCategory::Special, Some(25)));
    assert_eq!(mv.targeting, pkrs_data::MoveTargetScope::SingleFoe);
    assert!(mv.flags.is_empty() && mv.script.is_none());
    let legacy =
        MoveDef::parse_ron(include_str!("fixtures/growth.ron"), "growth.ron").expect("parse");
    assert_eq!(legacy.pp, None, "pp is optional");
}

#[test]
fn battle_pack_rejects_duplicate_move_id() {
    let chart = TypeChart::from_parts(vec!["normal".into()], Default::default());
    let mut pack = BattlePack::new(chart);
    let mv = MoveDef::parse_ron(include_str!("fixtures/growth.ron"), "growth.ron").expect("parse");
    pack.insert_move(mv.clone()).expect("first insert");
    let err = pack.insert_move(mv).unwrap_err();
    assert!(matches!(
        err,
        DataError::Duplicate { kind, id } if kind == "move" && id == "growth"
    ));
}
