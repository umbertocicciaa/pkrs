//! Scripted AI port (§7.5): view contents per visibility, choice resolution and fallback to
//! the built-in tier on script errors and invalid actions.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{battle, creature};
use pkrs_battle::{
    AiChoice, AiFallbackReason, AiMoveRef, AiRequest, AiScript, AiScriptView, AiView, Battle,
    BattleAction, BattleAi, BattleEvent, SideId, SlotRef, TrainerAi,
};
use pkrs_data::{AiProfile, AiTier, AiVisibility};

/// Replies with a fixed sequence of results and remembers the views it was shown.
struct Canned {
    replies: Vec<Result<AiChoice, String>>,
    seen: Vec<AiScriptView>,
}

impl Canned {
    fn new(replies: Vec<Result<AiChoice, String>>) -> Self {
        Self {
            replies,
            seen: Vec::new(),
        }
    }
}

impl AiScript for Canned {
    fn choose(&mut self, view: &AiScriptView) -> Result<AiChoice, String> {
        self.seen.push(view.clone());
        if self.replies.is_empty() {
            return Ok(AiChoice::Default);
        }
        self.replies.remove(0)
    }
}

fn profile(visibility: AiVisibility, allow_items: bool) -> AiProfile {
    AiProfile {
        visibility,
        allow_items,
        ..AiProfile::builtin("scripted", AiTier::Basic)
    }
}

/// Player: kit (normal; tackle, ember with 0 PP, water_gun) + bench sprout.
/// Opponent: leaf (grass) + bench drip (unrevealed).
fn scene() -> Battle {
    let mut kit = creature("kit", "normal", 80, 30, &["tackle", "ember", "water_gun"]);
    if let Some(m) = kit.moves[1].as_mut() {
        m.pp = 0;
    }
    let sprout = creature("sprout", "grass", 60, 10, &["vine_whip"]);
    let leaf = creature("leaf", "grass", 70, 20, &["vine_whip", "tackle"]);
    let drip = creature("drip", "water", 70, 20, &["water_gun"]);
    battle(vec![kit, sprout], vec![leaf, drip], 4)
}

fn ai_view<'a>(snapshot: &'a pkrs_battle::BattleView, b: &'a Battle) -> AiView<'a> {
    AiView {
        battle: snapshot,
        side: SideId::Player,
        pack: &b.resources().pack,
    }
}

fn scripted(
    replies: Vec<Result<AiChoice, String>>,
    allow_items: bool,
) -> pkrs_battle::ScriptedAi<Canned> {
    pkrs_battle::ScriptedAi::new(
        &profile(AiVisibility::Fair, allow_items),
        vec!["potion".into()],
        9,
        SideId::Player,
        Canned::new(replies),
    )
}

fn tier_choice(b: &Battle, allow_items: bool) -> BattleAction {
    let snapshot = b.view();
    let mut tier = TrainerAi::new(
        &profile(AiVisibility::Fair, allow_items),
        vec!["potion".into()],
        9,
    );
    tier.choose_action(&ai_view(&snapshot, b), 0)
}

fn mv(id: &str) -> AiChoice {
    AiChoice::Move {
        mv: AiMoveRef::Id(id.into()),
        target: None,
    }
}

#[test]
fn fair_view_hides_the_unrevealed_bench_stats_and_moves() {
    let b = scene();
    let snapshot = b.view();
    let mut ai = scripted(vec![], false);
    let view = ai.script_view(&ai_view(&snapshot, &b), AiRequest::Action { slot: 0 });

    assert_eq!(view.visibility, AiVisibility::Fair);
    assert_eq!(view.me().unwrap().name, "kit");
    assert_eq!(view.own.party.len(), 2, "own party fully visible");
    assert_eq!(view.own.party[0].hp, Some(80));
    assert_eq!(view.own.party[0].moves[1].pp, Some(0));
    assert!(
        view.items.is_empty(),
        "items hidden when the profile forbids them"
    );

    assert_eq!(view.foe.party_size, 2);
    assert_eq!(view.foe.party.len(), 1, "drip has not been sent out");
    let leaf = view.foe.active_creature(0).unwrap();
    assert_eq!((leaf.name.as_str(), leaf.slot), ("leaf", Some(0)));
    assert_eq!(leaf.hp_percent, 100);
    assert_eq!(
        (leaf.hp, leaf.max_hp, leaf.stats.clone()),
        (None, None, None)
    );
    assert!(leaf.moves.is_empty(), "no move revealed yet");

    // Once leaf uses vine_whip and drip is switched in, both are known (but not tackle).
    ai.observe(&[
        BattleEvent::MoveUsed {
            user_side: SideId::Opponent,
            user_slot: 0,
            move_id: "vine_whip".into(),
            target_side: SideId::Player,
            target_slot: 0,
        },
        BattleEvent::Switched {
            side: SideId::Opponent,
            slot: 1,
            to_party: 1,
        },
    ]);
    let view = ai.script_view(&ai_view(&snapshot, &b), AiRequest::Action { slot: 0 });
    assert_eq!(view.foe.party.len(), 2);
    let leaf = view.foe.member(0).unwrap();
    let ids: Vec<&str> = leaf.moves.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["vine_whip"]);
    assert_eq!(leaf.moves[0].pp, None, "PP stays hidden");
    assert_eq!(leaf.moves[0].def.as_ref().unwrap().power, 45);
    assert_eq!(
        view.foe.member(1).unwrap().slot,
        None,
        "drip is benched again"
    );
}

#[test]
fn full_view_shows_the_whole_foe_party() {
    let b = scene();
    let snapshot = b.view();
    let mut ai = pkrs_battle::ScriptedAi::new(
        &profile(AiVisibility::Full, true),
        vec!["potion".into()],
        9,
        SideId::Player,
        Canned::new(vec![]),
    );
    let view = ai.script_view(&ai_view(&snapshot, &b), AiRequest::Replace { slot: 0 });
    assert_eq!(view.visibility, AiVisibility::Full);
    assert!(view.me().is_none(), "no acting creature when replacing");
    assert_eq!(view.items, ["potion"]);
    assert_eq!(view.foe.party.len(), 2);
    let drip = view.foe.member(1).unwrap();
    assert_eq!(drip.hp, Some(70));
    assert_eq!(drip.stats.as_ref().unwrap().speed, 20);
    assert_eq!(drip.moves[0].pp, Some(20));
}

#[test]
fn valid_choices_become_battle_actions() {
    let b = scene();
    let snapshot = b.view();
    let v = ai_view(&snapshot, &b);
    let mut ai = scripted(
        vec![
            Ok(mv("water_gun")),
            Ok(AiChoice::Move {
                mv: AiMoveRef::Slot(0),
                target: Some(SlotRef::new(SideId::Player, 0)),
            }),
            Ok(AiChoice::Switch { to_party: 1 }),
            Ok(AiChoice::Item {
                item_id: "potion".into(),
                target_slot: None,
            }),
        ],
        true,
    );
    assert_eq!(
        ai.choose_action(&v, 0),
        BattleAction::Fight {
            move_slot: 2,
            target_side: SideId::Opponent,
            target_slot: 0
        }
    );
    assert_eq!(
        ai.choose_action(&v, 0),
        BattleAction::Fight {
            move_slot: 0,
            target_side: SideId::Player,
            target_slot: 0
        },
        "a script may target its own side"
    );
    assert_eq!(
        ai.choose_action(&v, 0),
        BattleAction::Switch { to_party: 1 }
    );
    assert_eq!(
        ai.choose_action(&v, 0),
        BattleAction::Item {
            item_id: "potion".into(),
            target_side: SideId::Player,
            target_slot: 0
        }
    );
    assert!(ai.fallback_ai().items().is_empty(), "the potion is used up");
    assert!(ai.fallbacks().is_empty());
}

#[test]
fn invalid_choices_fall_back_to_the_tier() {
    let b = scene();
    let snapshot = b.view();
    let v = ai_view(&snapshot, &b);
    let invalid = vec![
        mv("ember"),      // no PP
        mv("hyper_beam"), // not known
        AiChoice::Move {
            mv: AiMoveRef::Slot(3), // empty slot
            target: None,
        },
        AiChoice::Move {
            mv: AiMoveRef::Slot(0),
            target: Some(SlotRef::new(SideId::Opponent, 1)), // out of range in singles
        },
        AiChoice::Switch { to_party: 0 }, // already active
        AiChoice::Switch { to_party: 7 }, // no such member
        AiChoice::Item {
            item_id: "potion".into(), // profile forbids items
            target_slot: None,
        },
    ];
    let n = invalid.len();
    let mut ai = scripted(invalid.into_iter().map(Ok).collect(), false);
    let expected = tier_choice(&b, false);
    for _ in 0..n {
        assert_eq!(ai.choose_action(&v, 0), expected);
    }
    assert_eq!(ai.fallbacks().len(), n);
    for f in ai.fallbacks() {
        assert!(
            matches!(f.reason, AiFallbackReason::InvalidAction(_)),
            "{f:?}"
        );
        assert_eq!(f.request, AiRequest::Action { slot: 0 });
    }
}

#[test]
fn script_errors_and_defaults_use_the_tier() {
    let b = scene();
    let snapshot = b.view();
    let v = ai_view(&snapshot, &b);
    let mut ai = scripted(vec![Err("boom".into()), Ok(AiChoice::Default)], false);
    let expected = tier_choice(&b, false);
    assert_eq!(ai.choose_action(&v, 0), expected);
    assert_eq!(ai.choose_action(&v, 0), expected);
    assert_eq!(
        ai.fallbacks().len(),
        1,
        "a deliberate `nil` is not a failure"
    );
    assert_eq!(
        ai.fallbacks()[0].reason,
        AiFallbackReason::ScriptError("boom".into())
    );
}

#[test]
fn replacements_must_be_legal_switches() {
    let b = scene();
    let snapshot = b.view();
    let v = ai_view(&snapshot, &b);
    let mut ai = scripted(
        vec![
            Ok(AiChoice::Switch { to_party: 0 }),
            Ok(mv("tackle")),
            Ok(AiChoice::Switch { to_party: 1 }),
        ],
        false,
    );
    let tier = {
        let mut t = TrainerAi::new(&profile(AiVisibility::Fair, false), vec![], 9);
        t.choose_replacement(&v, 0)
    };
    assert_eq!(ai.choose_replacement(&v, 0), tier);
    assert_eq!(ai.choose_replacement(&v, 0), tier);
    assert_eq!(ai.choose_replacement(&v, 0), 1);
    assert_eq!(ai.fallbacks().len(), 2);
    assert!(
        ai.fallbacks()
            .iter()
            .all(|f| f.request == AiRequest::Replace { slot: 0 })
    );
}
