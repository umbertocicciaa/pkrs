//! Integration (§7.5 Lua AI, Plugin AI): a trainer driven by a Lua `choose_action` fixture
//! against the built-in smart AI, snapshot-tested and replay-checked.
//!
//! Regenerate the snapshot with `PKRS_UPDATE_SNAPSHOTS=1 cargo test -p pkrs-script --test lua_ai`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::Path;

use pkrs_battle::{
    AiFallbackReason, Battle, BattleConfig, BattleCreature, BattleEvent, BattleInput, BattlePhase,
    BattleResources, BattleResult, BattleSetup, BattleSideSetup, CreatureStats, MoveSlot,
    NullBattleHooks, RngPolicy, ScriptedAi, SideId, TrainerAi, ai_side_input,
};
use pkrs_core::Pcg64Rng;
use pkrs_data::{
    AiProfile, AiTier, AiVisibility, BattlePack, MoveCategory, MoveDef, MoveEffect, TypeChart,
};
use pkrs_script::{LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner};

const POLICY: RngPolicy = RngPolicy::Fixed { seed: 20_261_106 };
const CHAMPION: &str = include_str!("fixtures/ai_champion.lua");
const SCRIPT: &str = "scripts/ai/champion.lua";

fn resources() -> BattleResources {
    let types = ["normal", "fire", "water", "grass"]
        .map(String::from)
        .to_vec();
    let mut eff = HashMap::new();
    for (atk, def, m) in [
        ("fire", "grass", 2.0),
        ("fire", "water", 0.5),
        ("fire", "fire", 0.5),
        ("water", "fire", 2.0),
        ("water", "grass", 0.5),
        ("water", "water", 0.5),
        ("grass", "water", 2.0),
        ("grass", "fire", 0.5),
        ("grass", "grass", 0.5),
    ] {
        eff.insert((atk.to_string(), def.to_string()), m);
    }
    let mut pack = BattlePack::new(TypeChart::from_parts(types, eff));
    for (id, ty, power) in [
        ("tackle", "normal", 40),
        ("ember", "fire", 40),
        ("water_gun", "water", 40),
        ("vine_whip", "grass", 45),
    ] {
        pack.insert_move(MoveDef {
            id: id.into(),
            type_id: ty.into(),
            category: MoveCategory::Physical,
            power,
            accuracy: 100,
            priority: 0,
            effects: vec![MoveEffect::Damage],
        })
        .unwrap();
    }
    BattleResources::new(pack)
}

fn creature(name: &str, ty: &str, hp: u32, speed: u32, moves: &[&str]) -> BattleCreature {
    let mut slots: [Option<MoveSlot>; 4] = [None, None, None, None];
    for (i, id) in moves.iter().enumerate() {
        slots[i] = Some(MoveSlot {
            id: (*id).into(),
            priority: 0,
            power: 40,
            pp: 20,
            max_pp: 20,
        });
    }
    BattleCreature {
        species_id: name.into(),
        types: vec![ty.into()],
        nickname: name.into(),
        level: 20,
        stats: CreatureStats::new(hp, 30, 25, speed),
        moves: slots,
    }
}

fn setup() -> BattleSetup {
    let side = |name: &str, party| BattleSideSetup {
        name: name.into(),
        party,
        leading: vec![Some(0)],
    };
    BattleSetup {
        player: side(
            "Aria",
            vec![
                creature("torch", "fire", 70, 30, &["ember", "tackle"]),
                creature("splash", "water", 75, 22, &["water_gun", "tackle"]),
                creature("sprout", "grass", 80, 18, &["vine_whip", "tackle"]),
            ],
        ),
        opponent: side(
            "Champion",
            vec![
                creature("sprig", "grass", 72, 26, &["vine_whip", "tackle"]),
                creature("flare", "fire", 68, 28, &["ember", "tackle"]),
                creature("drip", "water", 78, 20, &["water_gun", "tackle"]),
            ],
        ),
    }
}

fn new_battle() -> Battle {
    Battle::new(
        setup(),
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

fn host(champion_source: &str) -> LuaScriptHost {
    let files = MemoryScripts::new().with(SCRIPT, champion_source);
    let mut h = LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(1)),
        ScriptLimits::default(),
    )
    .unwrap();
    h.load_ai_script(&ScriptOwner::Game, SCRIPT).unwrap();
    h
}

fn champion_profile() -> AiProfile {
    AiProfile::parse_ron(
        r#"AiProfile(
          id: "champion_lua",
          tier: Smart,
          script: "scripts/ai/champion.lua",
          allow_items: true,
          heal_below_percent: 30,
        )"#,
        "profiles/champion_lua.ron",
    )
    .unwrap()
}

struct Run {
    inputs: Vec<BattleInput>,
    events: Vec<BattleEvent>,
    result: BattleResult,
    fallbacks: Vec<AiFallbackReason>,
}

fn run(h: &LuaScriptHost, profile: &AiProfile) -> Run {
    let mut player = TrainerAi::for_battle(
        &AiProfile::builtin("smart", AiTier::Smart),
        vec![],
        &POLICY,
        SideId::Player,
    );
    let script = h.lua_ai(profile, &POLICY, SideId::Opponent).unwrap();
    let mut champion = ScriptedAi::for_battle(
        profile,
        vec!["potion".into()],
        &POLICY,
        SideId::Opponent,
        script,
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
            opponent: ai_side_input(&mut champion, &battle, SideId::Opponent),
        };
        inputs.push(input.clone());
        let step = battle.step(input, &mut NullBattleHooks).unwrap();
        champion.observe(&step);
        events.extend(step);
    }
    assert_eq!(battle.phase(), BattlePhase::End, "battle must finish");
    Run {
        inputs,
        events,
        result: *battle.result().unwrap(),
        fallbacks: champion
            .fallbacks()
            .iter()
            .map(|f| f.reason.clone())
            .collect(),
    }
}

fn render(run: &Run) -> String {
    let mut out = String::new();
    for (i, input) in run.inputs.iter().enumerate() {
        if let Some(choice) = &input.opponent {
            out.push_str(&format!("champion[{i}]: {:?}\n", choice.slots));
        }
    }
    for e in &run.events {
        out.push_str(&format!("{e:?}\n"));
    }
    out.push_str(&format!("result: {:?}\n", run.result));
    out
}

#[test]
fn lua_trainer_matches_snapshot() {
    let h = host(CHAMPION);
    let run = run(&h, &champion_profile());
    assert!(run.fallbacks.is_empty(), "{:?}", run.fallbacks);
    let rendered = render(&run);
    assert!(
        rendered.contains("Switch") && rendered.contains("Fight"),
        "the fixture both attacks and switches"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/lua_ai.snap");
    if std::env::var_os("PKRS_UPDATE_SNAPSHOTS").is_some() {
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
        "Lua AI log changed"
    );
}

#[test]
fn lua_ai_decisions_are_replay_stable() {
    let first = run(&host(CHAMPION), &champion_profile());
    // A fresh host whose shared script RNG has been advanced decides identically.
    let mut other = host(CHAMPION);
    other
        .run_source(
            &ScriptOwner::Game,
            "noise.lua",
            "for i = 1, 25 do math.random() end",
        )
        .unwrap();
    let second = run(&other, &champion_profile());
    assert_eq!(first.inputs, second.inputs, "same seed, same decisions");
    assert_eq!(first.events, second.events);

    // The recorded inputs replay without any Lua.
    let mut replay = new_battle();
    let mut events = Vec::new();
    for input in &first.inputs {
        events.extend(replay.step(input.clone(), &mut NullBattleHooks).unwrap());
    }
    assert_eq!(events, first.events);
    assert_eq!(replay.result(), Some(&first.result));
}

#[test]
fn a_broken_script_falls_back_to_the_tier_and_the_battle_finishes() {
    let h = host("function choose_action(view) if view.turn > 1 then error('boom') end end");
    let run = run(&h, &champion_profile());
    assert!(!run.fallbacks.is_empty());
    assert!(
        run.fallbacks.iter().all(
            |r| matches!(r, AiFallbackReason::ScriptError(m) if m.contains("boom")
                || m.contains("disabled"))
        ),
        "{:?}",
        run.fallbacks
    );
}

#[test]
fn a_plugin_profile_drives_a_trainer() {
    let mut h = host(CHAMPION);
    h.run_source(
        &ScriptOwner::plugin("gauntlet"),
        "plugins/gauntlet/main.lua",
        "ai.register_profile('gauntlet:boss', function(view)
           if view.request == 'replace' then return nil end
           -- Cheating boss: it sees the player's whole bench from turn 1.
           assert(#view.foe.party == view.foe.party_size, 'full visibility')
           assert(view.foe.party[2].hp ~= nil, 'exact HP')
           return { move = 'tackle' }
         end, { tier = 'basic', visibility = 'full' })",
    )
    .unwrap();
    let profile = h.registered_ai_profile("gauntlet:boss").unwrap();
    assert_eq!(profile.visibility, AiVisibility::Full);
    let run = run(&h, &profile);
    assert!(run.fallbacks.is_empty(), "{:?}", run.fallbacks);
    let tackles = run
        .events
        .iter()
        .filter(|e| {
            matches!(e, BattleEvent::MoveUsed { user_side: SideId::Opponent, move_id, .. }
            if move_id == "tackle")
        })
        .count();
    let opponent_moves = run
        .events
        .iter()
        .filter(|e| {
            matches!(
                e,
                BattleEvent::MoveUsed {
                    user_side: SideId::Opponent,
                    ..
                }
            )
        })
        .count();
    assert!(tackles > 0);
    assert_eq!(tackles, opponent_moves, "every move came from the plugin");
}
