//! Post-battle flow tests: default exp, distribution, level ups, the keyboard-driven
//! move-forget prompt and evolutions, on the Null adapters.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{BattleOutcome, BattleResult, battle_testkit};
use pkrs_data::{BattlePack, SpeciesDef, TypeChart};
use pkrs_platform::{
    DisplaySettings, FixedPaths, Input, InputFacadeConfig, Key, NullFrame, NullPlatform, Platform,
    PlatformConfig, block_on,
};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};

use super::*;
use crate::party::PartyMember;
use crate::ui_assets::build_test_ui_theme;

fn renderer() -> Renderer {
    Renderer::new(
        Box::new(NullRenderer::new()),
        DisplaySettings::default(),
        RendererConfig::default(),
    )
}

fn species(src: &str) -> SpeciesDef {
    SpeciesDef::parse_ron(src, "test.ron").unwrap()
}

fn pack() -> BattlePack {
    let mut pack = BattlePack::new(TypeChart::from_parts(Vec::new(), Default::default()));
    pack.insert_species(species(
        r#"(id: "sproutling", types: ["grass"], hp: 40, attack: 50, defense: 30, speed: 25,
            exp_yield: 64, growth: MediumFast,
            learnset: [(level: 1, move: "tackle"), (level: 6, move: "vine_whip"),
                       (level: 7, move: "leafage")],
            evolutions: [(into: "bloomling", trigger: Level(7))])"#,
    ))
    .unwrap();
    pack.insert_species(species(
        r#"(id: "bloomling", types: ["grass"], hp: 60, attack: 70, defense: 50, speed: 40,
            learnset: [(level: 7, move: "razor_leaf")])"#,
    ))
    .unwrap();
    pack.insert_species(species(
        r#"(id: "pebblit", types: ["normal"], hp: 20, attack: 20, defense: 20, speed: 10,
            exp_yield: 50)"#,
    ))
    .unwrap();
    pack
}

fn member(level: u8, moves: &[&str]) -> PartyMember {
    let pack = pack();
    PartyMember::new(
        &pack.species["sproutling"],
        level,
        moves.iter().map(|m| (*m).to_owned()).collect(),
    )
}

fn win(exp: u32) -> BattleResult {
    BattleResult {
        outcome: BattleOutcome::PlayerWin,
        turns: 2,
        exp,
        money: 0,
    }
}

fn flow(result: &BattleResult, earners: &[u8], party: &PlayerParty) -> Option<PostBattle> {
    let mut r = renderer();
    let theme = build_test_ui_theme(&mut r).unwrap();
    PostBattle::new(result, earners, party, &pack(), theme, r.view_size())
}

/// One tick with `keys` held; renders the frame too.
fn tick(flow: &mut PostBattle, input: &mut Input, keys: &[Key]) -> bool {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([NullFrame::with_keys(keys.iter().copied())])),
        Box::new(FixedPaths::under("pkrs-rewards-test")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    input.poll(&platform);
    input.begin_tick();
    let done = flow.update(input);
    let mut r = renderer();
    r.begin_frame(240, 160);
    flow.render(&mut r);
    r.end_frame();
    done
}

fn press(flow: &mut PostBattle, input: &mut Input, key: Key) -> bool {
    let a = tick(flow, input, &[key]);
    let b = tick(flow, input, &[]);
    a || b
}

/// Confirms texts until a menu opens or the flow ends; returns the texts read.
fn read(flow: &mut PostBattle, input: &mut Input) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..2_000 {
        if flow.is_done() || !flow.menu_labels().is_empty() {
            return seen;
        }
        if let Some(text) = flow.text() {
            if seen.last().map(String::as_str) != Some(text) {
                seen.push(text.to_owned());
            }
            press(flow, input, Key::Enter);
        } else {
            tick(flow, input, &[]);
        }
    }
    panic!("the flow did not settle: {seen:?}");
}

fn input() -> Input {
    Input::new(InputFacadeConfig::default())
}

#[test]
fn default_rewards_add_up_every_opponent() {
    let mut setup = battle_testkit::wild_1v1();
    setup.opponent.party[0].species_id = "pebblit".into();
    let mut second = setup.opponent.party[0].clone();
    second.level = 7;
    setup.opponent.party.push(second);
    let pack = pack();
    // 50·5/7 = 35 and 50·7/7 = 50.
    assert_eq!(battle_rewards(&BattleKind::Wild, &setup, &pack).exp, 85);
    let trainer = BattleKind::Trainer {
        name: "Coach".into(),
    };
    // ×1.5 each: 52 + 75.
    assert_eq!(battle_rewards(&trainer, &setup, &pack).exp, 127);
    // Unknown species give nothing.
    assert_eq!(
        battle_rewards(&BattleKind::Wild, &battle_testkit::wild_1v1(), &pack),
        BattleRewards::default()
    );
}

#[test]
fn nothing_to_show_without_a_win_exp_or_earner() {
    let party = PlayerParty::new(vec![member(5, &["tackle"])]);
    assert!(flow(&win(0), &[0], &party).is_none());
    let loss = BattleResult {
        outcome: BattleOutcome::OpponentWin,
        ..win(100)
    };
    assert!(flow(&loss, &[0], &party).is_none());
    assert!(flow(&win(100), &[], &party).is_none());
    assert!(
        flow(&win(100), &[3], &party).is_none(),
        "out-of-range earners are ignored"
    );
}

#[test]
fn exp_is_split_between_earners_without_level_up() {
    let party = PlayerParty::new(vec![member(5, &["tackle"]), member(5, &["tackle"])]);
    let mut f = flow(&win(51), &[0, 1], &party).unwrap();
    let mut input = input();
    let texts = read(&mut f, &mut input);
    assert_eq!(
        texts,
        [
            "Sproutling gained 26 Exp. Points!",
            "Sproutling gained 25 Exp. Points!"
        ]
    );
    assert!(f.is_done());
    assert_eq!(f.phase(), PostBattlePhase::Rewards);
    let party = f.into_party();
    assert_eq!(party.members()[0].exp, 125 + 26);
    assert_eq!(party.members()[1].exp, 125 + 25);
}

#[test]
fn level_up_learns_into_a_free_slot() {
    let party = PlayerParty::new(vec![member(5, &["tackle"])]);
    let mut f = flow(&win(100), &[0], &party).unwrap();
    let mut input = input();
    let texts = read(&mut f, &mut input);
    assert_eq!(
        texts,
        [
            "Sproutling gained 100 Exp. Points!",
            "Sproutling grew to level 6!",
            "Sproutling learned Vine whip!"
        ]
    );
    let m = &f.party().members()[0];
    assert_eq!((m.level, m.exp), (6, 225));
    assert_eq!(m.moves, ["tackle", "vine_whip"]);
}

#[test]
fn a_full_move_set_prompts_and_cancel_keeps_the_moves() {
    let moves = ["tackle", "scratch", "pound", "ember"];
    let party = PlayerParty::new(vec![member(5, &moves)]);
    let mut f = flow(&win(100), &[0], &party).unwrap();
    let mut input = input();
    let texts = read(&mut f, &mut input);
    assert_eq!(
        texts.last().map(String::as_str),
        Some("Sproutling wants to learn Vine whip, but Sproutling already knows 4 moves.")
    );
    assert_eq!(f.text(), Some("Which move should Sproutling forget?"));
    assert_eq!(
        f.menu_labels(),
        [
            "Tackle",
            "Scratch",
            "Pound",
            "Ember",
            "Don't learn Vine whip"
        ]
    );
    assert_eq!(f.menu_cursor(), Some(0));
    press(&mut f, &mut input, Key::X);
    assert!(f.menu_labels().is_empty());
    assert_eq!(
        read(&mut f, &mut input),
        ["Sproutling did not learn Vine whip."]
    );
    assert!(f.is_done());
    assert_eq!(f.party().members()[0].moves, moves);
}

#[test]
fn the_prompt_replaces_the_chosen_move_with_the_keyboard() {
    let party = PlayerParty::new(vec![member(5, &["tackle", "scratch", "pound", "ember"])]);
    let mut f = flow(&win(100), &[0], &party).unwrap();
    let mut input = input();
    read(&mut f, &mut input);
    press(&mut f, &mut input, Key::Down);
    press(&mut f, &mut input, Key::Down);
    assert_eq!(f.menu_cursor(), Some(2));
    press(&mut f, &mut input, Key::Enter);
    assert_eq!(
        read(&mut f, &mut input),
        [
            "1, 2 and... Poof!",
            "Sproutling forgot Pound and learned Vine whip!"
        ]
    );
    assert_eq!(
        f.party().members()[0].moves,
        ["tackle", "scratch", "vine_whip", "ember"]
    );
    // "Don't learn" (the last row) is the same as Cancel.
    let mut g = flow(&win(100), &[0], &party).unwrap();
    read(&mut g, &mut input);
    for _ in 0..4 {
        press(&mut g, &mut input, Key::Down);
    }
    press(&mut g, &mut input, Key::Enter);
    assert_eq!(
        read(&mut g, &mut input),
        ["Sproutling did not learn Vine whip."]
    );
}

#[test]
fn reaching_the_evolution_level_plays_the_evolution_scene() {
    let party = PlayerParty::new(vec![member(5, &["tackle"])]);
    // 125 + 218 = 343: level 7 (two levels at once).
    let mut f = flow(&win(218), &[0], &party).unwrap();
    let mut input = input();
    let texts = read(&mut f, &mut input);
    assert_eq!(
        texts,
        [
            "Sproutling gained 218 Exp. Points!",
            "Sproutling grew to level 6!",
            "Sproutling learned Vine whip!",
            "Sproutling grew to level 7!",
            "Sproutling learned Leafage!",
            "What? Sproutling is evolving!",
            "Congratulations! Your Sproutling evolved into Bloomling!",
            // The new species' moves at its level.
            "Bloomling learned Razor leaf!",
        ]
    );
    assert_eq!(f.phase(), PostBattlePhase::Evolution);
    let scene = f.scene().unwrap();
    assert!(scene.is_done() && scene.to() == "Bloomling");
    let m = &f.party().members()[0];
    assert_eq!((m.species.as_str(), m.level), ("bloomling", 7));
    assert_eq!(m.moves, ["tackle", "vine_whip", "leafage", "razor_leaf"]);
}

#[test]
fn the_evolution_waits_for_its_animation() {
    let party = PlayerParty::new(vec![member(6, &["tackle", "vine_whip"])]);
    let mut f = flow(&win(200), &[0], &party).unwrap();
    let mut input = input();
    let mut ticks_without_text = 0;
    for _ in 0..2_000 {
        if f.is_done() {
            break;
        }
        match f.text() {
            Some(_) => {
                press(&mut f, &mut input, Key::Enter);
            }
            None => {
                if f.phase() == PostBattlePhase::Evolution {
                    ticks_without_text += 1;
                }
                tick(&mut f, &mut input, &[]);
            }
        }
    }
    assert!(f.is_done());
    assert!(
        ticks_without_text >= crate::evolution::EVOLUTION_TICKS - 2,
        "{ticks_without_text}"
    );
    assert_eq!(f.into_party().members()[0].species, "bloomling");
}

#[test]
fn no_evolution_without_a_level_up() {
    // Already past the evolution level (e.g. evolution was skipped before): it only
    // evolves after a battle in which it levelled up.
    let party = PlayerParty::new(vec![member(8, &["tackle"])]);
    let mut f = flow(&win(1), &[0], &party).unwrap();
    let mut input = input();
    assert_eq!(
        read(&mut f, &mut input),
        ["Sproutling gained 1 Exp. Points!"]
    );
    assert_eq!(f.phase(), PostBattlePhase::Rewards);
    assert_eq!(f.party().members()[0].species, "sproutling");
}
