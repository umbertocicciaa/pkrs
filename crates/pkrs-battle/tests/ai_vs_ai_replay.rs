//! Integration (§7.5): AI vs AI battle, snapshot of its event log, and replay stability.
//!
//! Regenerate the snapshot with `PKRS_UPDATE_SNAPSHOTS=1 cargo test -p pkrs-battle --test ai_vs_ai_replay`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use common::{creature, resources, side};
use pkrs_battle::{
    Battle, BattleConfig, BattleEvent, BattleInput, BattlePhase, BattleResult, BattleSetup,
    NullBattleHooks, RngPolicy, SideId, TrainerAi, ai_side_input,
};
use pkrs_data::{AiProfile, AiTier};

const POLICY: RngPolicy = RngPolicy::Fixed { seed: 20_261_105 };

fn setup() -> BattleSetup {
    BattleSetup {
        player: side(
            "Aria",
            vec![
                creature("torch", "fire", 70, 30, &["ember", "tackle"]),
                creature("splash", "water", 75, 22, &["water_gun", "tackle"]),
                creature("sprout", "grass", 80, 18, &["vine_whip", "quick_attack"]),
            ],
        ),
        opponent: side(
            "Rival",
            vec![
                creature("sprig", "grass", 72, 26, &["vine_whip", "tackle"]),
                creature("flare", "fire", 68, 28, &["ember", "growl"]),
                creature("drip", "water", 78, 20, &["water_gun", "quick_attack"]),
            ],
        ),
    }
}

fn new_battle() -> Battle {
    Battle::new(
        setup(),
        // Pin tracing off so the snapshot does not depend on the `hook-trace` feature.
        BattleConfig {
            trace_hooks: false,
            ..BattleConfig::with_side_size(1)
        },
        resources(),
        POLICY,
        &mut NullBattleHooks,
    )
    .unwrap()
}

struct Run {
    inputs: Vec<BattleInput>,
    events: Vec<BattleEvent>,
    result: BattleResult,
}

fn ai_vs_ai() -> Run {
    let mut player = TrainerAi::for_battle(
        &AiProfile::builtin("smart", AiTier::Smart),
        vec![],
        &POLICY,
        SideId::Player,
    );
    let mut opponent = TrainerAi::for_battle(
        &AiProfile::builtin("basic", AiTier::Basic),
        vec![],
        &POLICY,
        SideId::Opponent,
    );
    let mut battle = new_battle();
    let mut inputs = Vec::new();
    let mut events = Vec::new();
    for _ in 0..500 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        let input = BattleInput {
            player: ai_side_input(&mut player, &battle, SideId::Player),
            opponent: ai_side_input(&mut opponent, &battle, SideId::Opponent),
        };
        inputs.push(input.clone());
        events.extend(battle.step(input, &mut NullBattleHooks).unwrap());
    }
    assert_eq!(battle.phase(), BattlePhase::End, "battle must finish");
    Run {
        inputs,
        events,
        result: *battle.result().unwrap(),
    }
}

fn render(run: &Run) -> String {
    let mut out = String::new();
    for e in &run.events {
        out.push_str(&format!("{e:?}\n"));
    }
    out.push_str(&format!("result: {:?}\n", run.result));
    out
}

#[test]
fn ai_vs_ai_matches_snapshot() {
    let rendered = render(&ai_vs_ai());
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/ai_vs_ai.snap");
    if std::env::var_os("PKRS_UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing snapshot {}; run with PKRS_UPDATE_SNAPSHOTS=1",
            path.display()
        )
    });
    assert_eq!(
        rendered,
        expected.replace("\r\n", "\n"),
        "AI vs AI log changed"
    );
}

#[test]
fn ai_decisions_are_replay_stable() {
    let first = ai_vs_ai();
    let second = ai_vs_ai();
    assert_eq!(first.inputs, second.inputs, "same seed, same decisions");
    assert_eq!(first.events, second.events);

    // Replaying only the recorded inputs, with no AI, reproduces the battle exactly: the AI
    // rolls on its own stream, never on the battle's.
    let mut replay = new_battle();
    let mut events = Vec::new();
    for input in &first.inputs {
        events.extend(replay.step(input.clone(), &mut NullBattleHooks).unwrap());
    }
    assert_eq!(events, first.events);
    assert_eq!(replay.result(), Some(&first.result));
}

#[test]
fn ai_streams_differ_per_side_and_seed() {
    let a = RngPolicy::Fixed { seed: 1 };
    let b = RngPolicy::Fixed { seed: 2 };
    assert_ne!(a.ai_seed(SideId::Player), a.ai_seed(SideId::Opponent));
    assert_ne!(a.ai_seed(SideId::Player), b.ai_seed(SideId::Player));
    assert_eq!(a.ai_seed(SideId::Player), a.ai_seed(SideId::Player));
}
