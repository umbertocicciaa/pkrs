//! AI tier choices on crafted scenarios (§7.5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{battle, creature};
use pkrs_battle::{AiView, BattleAction, BattleAi, BattleView, SideId, TrainerAi};
use pkrs_data::{AiProfile, AiTier, AiWeights};

fn profile(tier: AiTier, switch: f32) -> AiProfile {
    AiProfile {
        weights: AiWeights {
            switch,
            ..AiWeights::default()
        },
        ..AiProfile::builtin("test", tier)
    }
}

fn choose(ai: &mut TrainerAi, view: &BattleView, battle: &pkrs_battle::Battle) -> BattleAction {
    let v = AiView {
        battle: view,
        side: SideId::Player,
        pack: &battle.resources().pack,
    };
    ai.choose_action(&v, 0)
}

fn move_slot(action: &BattleAction) -> u8 {
    match action {
        BattleAction::Fight { move_slot, .. } => *move_slot,
        other => panic!("expected a move, got {other:?}"),
    }
}

#[test]
fn basic_picks_the_super_effective_move() {
    let b = battle(
        vec![creature(
            "kit",
            "normal",
            80,
            30,
            &["tackle", "ember", "water_gun"],
        )],
        vec![creature("sprout", "grass", 80, 10, &["tackle"])],
        1,
    );
    let mut ai = TrainerAi::new(&profile(AiTier::Basic, 0.5), vec![], 9);
    assert_eq!(
        move_slot(&choose(&mut ai, &b.view(), &b)),
        1,
        "ember vs grass"
    );
}

#[test]
fn basic_avoids_immune_and_status_moves() {
    let b = battle(
        vec![creature(
            "kit",
            "normal",
            80,
            30,
            &["tackle", "growl", "vine_whip"],
        )],
        vec![creature("shade", "ghost", 80, 10, &["tackle"])],
        1,
    );
    let mut ai = TrainerAi::new(&profile(AiTier::Basic, 0.5), vec![], 9);
    assert_eq!(
        move_slot(&choose(&mut ai, &b.view(), &b)),
        2,
        "vine whip; tackle can't hit ghosts"
    );
}

#[test]
fn smart_switches_when_walled() {
    let b = battle(
        vec![
            creature("brawler", "normal", 80, 30, &["tackle"]),
            creature("torch", "fire", 80, 20, &["ember"]),
        ],
        vec![creature("shade", "ghost", 80, 10, &["tackle"])],
        1,
    );
    let mut eager = TrainerAi::new(&profile(AiTier::Smart, 1.0), vec![], 9);
    assert_eq!(
        choose(&mut eager, &b.view(), &b),
        BattleAction::Switch { to_party: 1 }
    );
    let mut stubborn = TrainerAi::new(&profile(AiTier::Smart, 0.0), vec![], 9);
    assert_eq!(move_slot(&choose(&mut stubborn, &b.view(), &b)), 0);
}

#[test]
fn smart_stays_in_a_good_matchup() {
    let b = battle(
        vec![
            creature("torch", "fire", 80, 30, &["ember"]),
            creature("splash", "water", 80, 20, &["water_gun"]),
        ],
        vec![creature("sprout", "grass", 80, 10, &["vine_whip"])],
        1,
    );
    let mut ai = TrainerAi::new(&profile(AiTier::Smart, 1.0), vec![], 9);
    assert_eq!(move_slot(&choose(&mut ai, &b.view(), &b)), 0);
}

#[test]
fn smart_prefers_a_priority_ko() {
    let b = battle(
        vec![creature(
            "kit",
            "normal",
            80,
            5,
            &["tackle", "quick_attack"],
        )],
        vec![creature("weak", "normal", 4, 50, &["tackle"])],
        1,
    );
    let mut ai = TrainerAi::new(&profile(AiTier::Smart, 0.5), vec![], 9);
    assert_eq!(
        move_slot(&choose(&mut ai, &b.view(), &b)),
        1,
        "quick attack KOs first"
    );
}

#[test]
fn random_tier_is_deterministic_and_only_uses_usable_moves() {
    let b = battle(
        vec![creature(
            "kit",
            "normal",
            80,
            30,
            &["tackle", "ember", "growl"],
        )],
        vec![creature("foe", "normal", 80, 10, &["tackle"])],
        1,
    );
    let view = b.view();
    let rolls = |seed| {
        let mut ai = TrainerAi::new(&profile(AiTier::Random, 0.5), vec![], seed);
        (0..32)
            .map(|_| move_slot(&choose(&mut ai, &view, &b)))
            .collect::<Vec<_>>()
    };
    let a = rolls(5);
    assert_eq!(a, rolls(5));
    assert!(a.iter().all(|&s| s < 3));
    assert!(a.contains(&0) && a.contains(&1) && a.contains(&2), "{a:?}");
}

#[test]
fn items_are_used_at_low_hp_only_when_allowed() {
    let b = battle(
        vec![creature("kit", "normal", 80, 30, &["tackle"])],
        vec![creature("foe", "normal", 80, 10, &["tackle"])],
        1,
    );
    let mut view = b.view();
    view.sides[0].party[0].hp = 10;
    let mut allowed = AiProfile {
        allow_items: true,
        ..profile(AiTier::Smart, 0.5)
    };
    let mut ai = TrainerAi::new(&allowed, vec!["potion".into()], 9);
    assert_eq!(
        choose(&mut ai, &view, &b),
        BattleAction::Item {
            item_id: "potion".into(),
            target_side: SideId::Player,
            target_slot: 0,
        }
    );
    assert!(ai.items().is_empty(), "the item is consumed");
    assert!(matches!(
        choose(&mut ai, &view, &b),
        BattleAction::Fight { .. }
    ));

    allowed.allow_items = false;
    let mut ai = TrainerAi::new(&allowed, vec!["potion".into()], 9);
    assert!(matches!(
        choose(&mut ai, &view, &b),
        BattleAction::Fight { .. }
    ));
}

#[test]
fn replacement_picks_the_best_matchup() {
    let b = battle(
        vec![
            creature("kit", "normal", 80, 30, &["tackle"]),
            creature("sprout", "grass", 80, 20, &["vine_whip"]),
            creature("splash", "water", 80, 20, &["water_gun"]),
        ],
        vec![creature("torch", "fire", 80, 10, &["ember"])],
        1,
    );
    let mut view = b.view();
    view.sides[0].party[0].hp = 0;
    let v = AiView {
        battle: &view,
        side: SideId::Player,
        pack: &b.resources().pack,
    };
    for tier in [AiTier::Basic, AiTier::Smart] {
        let mut ai = TrainerAi::new(&profile(tier, 0.5), vec![], 9);
        assert_eq!(ai.choose_replacement(&v, 0), 2, "{tier:?}: water vs fire");
    }
}
