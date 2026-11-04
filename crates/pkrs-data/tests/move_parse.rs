//! Move and effect RON parsing.

use pkrs_data::{MoveCategory, MoveDef, MoveEffect};

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
