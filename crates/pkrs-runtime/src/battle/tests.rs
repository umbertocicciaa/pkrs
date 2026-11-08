//! Battle screen tests: keyboard-driven battles on the Null adapters.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use pkrs_battle::{
    ActionMenu, BattleError, BattleHookCtx, BattleHooks, BattleOutcome, BattleResult, CustomAction,
    MenuOption, NullBattleHooks, RngPolicy, SideId, TrainerAi, battle_testkit,
};
use pkrs_data::{AiProfile, AiTier};
use pkrs_platform::{
    DisplaySettings, FixedPaths, Input, InputFacadeConfig, Key, NullFrame, NullPlatform, Platform,
    PlatformConfig, block_on,
};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};

use super::content::{BattleKind, PreparedBattle};
use super::present::Cue;
use super::screen::{BattleScreen, MenuKind};
use crate::ui_assets::build_test_ui_theme;

fn renderer() -> Renderer {
    Renderer::new(
        Box::new(NullRenderer::new()),
        DisplaySettings::default(),
        RendererConfig::default(),
    )
}

fn prepared(kind: BattleKind, setup: pkrs_battle::BattleSetup) -> PreparedBattle {
    let side_size = setup
        .player
        .leading
        .len()
        .max(setup.opponent.leading.len())
        .try_into()
        .unwrap_or(1);
    PreparedBattle {
        kind,
        config: pkrs_battle::BattleConfig::with_side_size(side_size),
        setup,
        resources: battle_testkit::default_resources(),
        ai: TrainerAi::new(&AiProfile::builtin("basic", AiTier::Basic), Vec::new(), 3),
        text: crate::locale::BattleText::default(),
    }
}

fn rival() -> BattleKind {
    BattleKind::Trainer {
        name: "Rival".into(),
    }
}

fn screen_with(
    kind: BattleKind,
    setup: pkrs_battle::BattleSetup,
    bag: BTreeMap<String, u32>,
    hooks: &mut dyn BattleHooks,
) -> BattleScreen {
    let mut r = renderer();
    let theme = build_test_ui_theme(&mut r).unwrap();
    BattleScreen::new(
        prepared(kind, setup),
        RngPolicy::Fixed { seed: 11 },
        theme,
        r.view_size(),
        bag,
        hooks,
    )
    .unwrap()
}

fn screen(kind: BattleKind) -> BattleScreen {
    screen_with(
        kind,
        battle_testkit::wild_1v1(),
        BTreeMap::new(),
        &mut NullBattleHooks,
    )
}

/// One tick with `keys` held.
fn tick(
    screen: &mut BattleScreen,
    input: &mut Input,
    keys: &[Key],
    hooks: &mut dyn BattleHooks,
) -> Option<BattleResult> {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([NullFrame::with_keys(keys.iter().copied())])),
        Box::new(FixedPaths::under("pkrs-battle-test")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    input.poll(&platform);
    input.begin_tick();
    let result = screen.update(input, hooks);
    let mut r = renderer();
    r.begin_frame(240, 160);
    screen.render(&mut r);
    r.end_frame();
    result
}

/// Presses `key` for one tick, then releases it for one tick.
fn press(
    screen: &mut BattleScreen,
    input: &mut Input,
    key: Key,
    hooks: &mut dyn BattleHooks,
) -> Option<BattleResult> {
    let a = tick(screen, input, &[key], hooks);
    let b = tick(screen, input, &[], hooks);
    a.or(b)
}

/// Confirms texts until a menu opens (or the battle ends); returns the texts read.
fn read_texts(
    screen: &mut BattleScreen,
    input: &mut Input,
    hooks: &mut dyn BattleHooks,
) -> (Vec<String>, Option<BattleResult>) {
    let mut seen = Vec::new();
    for _ in 0..2_000 {
        if screen.menu().is_some() {
            return (seen, None);
        }
        if let Some(text) = screen.text() {
            if seen.last().map(String::as_str) != Some(text) {
                seen.push(text.to_owned());
            }
            if let Some(result) = press(screen, input, Key::Enter, hooks) {
                return (seen, Some(result));
            }
        } else if let Some(result) = tick(screen, input, &[], hooks) {
            return (seen, Some(result));
        }
    }
    panic!("no menu and no result after 2000 iterations: {seen:?}");
}

fn input() -> Input {
    Input::new(InputFacadeConfig::default())
}

#[test]
fn keyboard_plays_a_trainer_battle_to_a_win() {
    let mut s = screen(rival());
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    let (intro, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(
        intro,
        ["Rival wants to battle!", "Rival sent out foe!", "Go! hero!"]
    );
    let mut all_texts = intro;
    let mut foe_hp_seen = Vec::new();
    let result = loop {
        assert_eq!(s.menu(), Some(MenuKind::Action));
        assert_eq!(s.menu_labels(), ["FIGHT", "BAG", "PARTY", "RUN"]);
        press(&mut s, &mut input, Key::Enter, hooks); // FIGHT
        assert_eq!(s.menu(), Some(MenuKind::Fight));
        let pp = s.battle().view().sides[0].party[0].moves[0]
            .as_ref()
            .unwrap()
            .pp;
        assert_eq!(s.menu_labels(), [format!("Tackle {pp}/35")]);
        press(&mut s, &mut input, Key::Enter, hooks); // Tackle
        // Play back the turn, sampling the foe's HP bar every tick.
        let mut result = None;
        for _ in 0..2_000 {
            if s.menu().is_some() {
                break;
            }
            if let Some(m) = s.displayed().side(SideId::Opponent).active_member(0) {
                foe_hp_seen.push(m.hp);
            }
            if let Some(text) = s.text() {
                if all_texts.last().map(String::as_str) != Some(text) {
                    all_texts.push(text.to_owned());
                }
                result = result.or(press(&mut s, &mut input, Key::Enter, hooks));
            } else {
                result = result.or(tick(&mut s, &mut input, &[], hooks));
            }
            if result.is_some() {
                break;
            }
        }
        if let Some(result) = result {
            break result;
        }
    };
    assert_eq!(result.outcome, BattleOutcome::PlayerWin);
    assert!(s.is_finished());
    assert!(all_texts.iter().any(|t| t == "hero used Tackle!"));
    assert!(all_texts.iter().any(|t| t == "Foe foe fainted!"));
    assert_eq!(all_texts.last().unwrap(), "Player defeated Rival!");
    // The bar animated through intermediate values, not in one jump.
    foe_hp_seen.dedup();
    assert!(foe_hp_seen.len() > 4, "{foe_hp_seen:?}");
    assert_eq!(*foe_hp_seen.last().unwrap(), 0);
}

#[test]
fn logic_runs_ahead_of_playback() {
    let mut s = screen(rival());
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    read_texts(&mut s, &mut input, hooks);
    press(&mut s, &mut input, Key::Enter, hooks); // FIGHT
    let turn = s.battle().turn();
    tick(&mut s, &mut input, &[Key::Enter], hooks); // Tackle: the whole turn resolves now
    assert!(s.menu().is_none());
    assert!(s.battle().turn() > turn, "the logic finished the turn");
    let logic_hp = s.battle().view().sides[1].party[0].hp;
    let shown_hp = s.displayed().side(SideId::Opponent).members[0].hp;
    assert!(logic_hp < shown_hp, "the HP bar has not moved yet");
    assert!(s.queued().iter().any(|c| matches!(c, Cue::Hp { .. })));
}

#[test]
fn cancel_goes_back_and_running_from_a_trainer_is_refused() {
    let mut s = screen(rival());
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    read_texts(&mut s, &mut input, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    assert_eq!(s.menu(), Some(MenuKind::Fight));
    press(&mut s, &mut input, Key::X, hooks);
    assert_eq!(s.menu(), Some(MenuKind::Action));
    for _ in 0..3 {
        press(&mut s, &mut input, Key::Down, hooks);
    }
    assert_eq!(s.menu_cursor(), Some(3));
    press(&mut s, &mut input, Key::Enter, hooks); // RUN
    let (texts, result) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts, ["There's no running from a trainer battle!"]);
    assert!(result.is_none());
    assert_eq!(s.menu(), Some(MenuKind::Action));
    // The bag is empty: a message, then the action menu again.
    press(&mut s, &mut input, Key::Down, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts, ["The bag is empty."]);
    assert_eq!(s.menu(), Some(MenuKind::Action));
}

#[test]
fn running_from_a_wild_battle_returns_fled() {
    let mut s = screen(BattleKind::Wild);
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    let (intro, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(intro, ["A wild foe appeared!", "Go! hero!"]);
    for _ in 0..3 {
        press(&mut s, &mut input, Key::Down, hooks);
    }
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, result) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts, ["Got away safely!"]);
    assert_eq!(
        result.unwrap().outcome,
        BattleOutcome::Fled {
            side: SideId::Player
        }
    );
}

#[test]
fn bag_items_submit_an_item_action() {
    let bag = BTreeMap::from([("potion".to_owned(), 1)]);
    let mut s = screen_with(
        rival(),
        battle_testkit::wild_1v1(),
        bag,
        &mut NullBattleHooks,
    );
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    read_texts(&mut s, &mut input, hooks);
    press(&mut s, &mut input, Key::Down, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    assert_eq!(s.menu(), Some(MenuKind::Bag));
    assert_eq!(s.menu_labels(), ["Potion x1"]);
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts[0], "Used Potion!");
}

/// Adds a `dance` entry and implements it.
#[derive(Default)]
struct DanceHooks {
    danced: u32,
}

impl BattleHooks for DanceHooks {
    fn on_action_menu(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        menu: &mut ActionMenu,
    ) -> Result<(), BattleError> {
        menu.options.insert(
            1,
            MenuOption {
                id: "dance".into(),
                label: "Dance".into(),
            },
        );
        Ok(())
    }

    fn on_custom_action(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        action: &mut CustomAction,
    ) -> Result<(), BattleError> {
        if action.id == "dance" {
            self.danced += 1;
            action.handled = true;
            ctx.message("text:It danced!");
        }
        Ok(())
    }
}

#[test]
fn hook_added_menu_entries_submit_custom_actions() {
    let mut hooks = DanceHooks::default();
    let mut s = screen_with(
        rival(),
        battle_testkit::wild_1v1(),
        BTreeMap::new(),
        &mut hooks,
    );
    let mut input = input();
    read_texts(&mut s, &mut input, &mut hooks);
    assert_eq!(s.menu_labels(), ["FIGHT", "Dance", "BAG", "PARTY", "RUN"]);
    press(&mut s, &mut input, Key::Down, &mut hooks);
    press(&mut s, &mut input, Key::Enter, &mut hooks);
    let (texts, _) = read_texts(&mut s, &mut input, &mut hooks);
    assert_eq!(hooks.danced, 1);
    assert!(texts.iter().any(|t| t == "It danced!"), "{texts:?}");
}

#[test]
fn a_fainted_lead_forces_a_switch_that_cannot_be_cancelled() {
    let mut setup = battle_testkit::wild_1v1();
    let lead = &mut setup.player.party[0];
    lead.stats.max_hp = 1;
    lead.stats.speed = 1;
    let mut backup = setup.player.party[0].clone();
    backup.nickname = "backup".into();
    backup.stats.max_hp = 200;
    backup.stats.attack = 200;
    setup.player.party.push(backup);
    let mut s = screen_with(rival(), setup, BTreeMap::new(), &mut NullBattleHooks);
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    read_texts(&mut s, &mut input, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, _) = read_texts(&mut s, &mut input, hooks);
    assert!(texts.iter().any(|t| t == "hero fainted!"), "{texts:?}");
    assert_eq!(s.menu(), Some(MenuKind::Party { forced: true }));
    press(&mut s, &mut input, Key::X, hooks);
    assert_eq!(s.menu(), Some(MenuKind::Party { forced: true }));
    // The fainted lead cannot go back in.
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts, ["hero has no energy left to battle!"]);
    press(&mut s, &mut input, Key::Down, hooks);
    press(&mut s, &mut input, Key::Enter, hooks);
    let (texts, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(texts, ["Go! backup!"]);
    assert_eq!(s.menu(), Some(MenuKind::Action));
    assert_eq!(
        s.displayed()
            .side(SideId::Player)
            .active_member(0)
            .unwrap()
            .name,
        "backup"
    );
}

#[test]
fn battle_state_pops_back_and_queues_the_result_for_the_script() {
    use crate::ctx::RuntimeCtx;
    use crate::state::{BattleState, GameState, StateStack, Transition};
    use pkrs_script::{ScriptValue, WakeSignal};

    struct Overworld;
    impl GameState for Overworld {
        fn id(&self) -> &str {
            "overworld"
        }
        fn update(&mut self, _: &mut RuntimeCtx) -> Transition {
            Transition::None
        }
        fn render(&self, _: &mut Renderer, _: f32) {}
    }

    let mut ctx = RuntimeCtx::default();
    let mut stack = StateStack::new();
    stack.push(&mut ctx, Box::new(Overworld));
    stack.push(
        &mut ctx,
        Box::new(BattleState::new(screen(BattleKind::Wild))),
    );
    assert_eq!(stack.top_id(), Some("battle"));
    // Mashing Confirm reads every text, picks FIGHT and the first move each turn.
    for i in 0..4_000 {
        if stack.top_id() != Some("battle") {
            break;
        }
        let keys: &[Key] = if i % 2 == 0 { &[Key::Enter] } else { &[] };
        let mut platform = Platform::new(
            Box::new(NullPlatform::new().with_frames([NullFrame::with_keys(keys.iter().copied())])),
            Box::new(FixedPaths::under("pkrs-battle-test")),
            PlatformConfig::default(),
        );
        block_on(platform.next_frame());
        ctx.input.poll(&platform);
        ctx.input.begin_tick();
        stack.run_update(&mut ctx);
        stack.apply_pending(&mut ctx);
    }
    assert_eq!(stack.top_id(), Some("overworld"));
    let wakes: Vec<WakeSignal> = ctx.drain_script_wakes().into_iter().collect();
    let [WakeSignal::BattleEnded(ScriptValue::Map(result))] = wakes.as_slice() else {
        panic!("expected one BattleEnded wake, got {wakes:?}");
    };
    assert_eq!(result.get("outcome"), Some(&ScriptValue::from("win")));
}

fn doubles_setup() -> pkrs_battle::BattleSetup {
    use pkrs_battle::{BattleCreature, BattleSideSetup, CreatureStats, MoveSlot};
    let mk = |name: &str, hp: u32, spe: u32| BattleCreature {
        species_id: name.into(),
        types: vec!["normal".into()],
        nickname: name.into(),
        level: 8,
        stats: CreatureStats::new(hp, 20, 10, spe),
        moves: [
            Some(MoveSlot {
                id: "tackle".into(),
                priority: 0,
                power: 35,
                pp: 35,
                max_pp: 35,
            }),
            None,
            None,
            None,
        ],
    };
    pkrs_battle::BattleSetup {
        player: BattleSideSetup {
            name: "Player".into(),
            party: vec![mk("a", 50, 15), mk("b", 50, 12)],
            leading: vec![Some(0), Some(1)],
        },
        opponent: BattleSideSetup {
            name: "Twins".into(),
            party: vec![mk("x", 25, 8), mk("y", 25, 6)],
            leading: vec![Some(0), Some(1)],
        },
    }
}

#[test]
fn doubles_two_active_slots_reopen_action_menu_after_one_turn() {
    let mut s = screen_with(
        rival(),
        doubles_setup(),
        BTreeMap::new(),
        &mut NullBattleHooks,
    );
    let mut input = input();
    let hooks = &mut NullBattleHooks;
    let (_, _) = read_texts(&mut s, &mut input, hooks);
    assert_eq!(s.battle().config().side_size, 2);
    for _slot in 0..2 {
        assert_eq!(s.menu(), Some(MenuKind::Action));
        press(&mut s, &mut input, Key::Enter, hooks); // FIGHT
        assert_eq!(s.menu(), Some(MenuKind::Fight));
        press(&mut s, &mut input, Key::Enter, hooks); // Tackle
        while s.menu() == Some(MenuKind::Target) {
            press(&mut s, &mut input, Key::Enter, hooks);
        }
    }
    for _ in 0..8_000 {
        if s.menu() == Some(MenuKind::Action) {
            return;
        }
        if s.is_finished() {
            return;
        }
        if s.text().is_some() {
            press(&mut s, &mut input, Key::Enter, hooks);
        } else {
            tick(&mut s, &mut input, &[], hooks);
        }
    }
    panic!(
        "expected action menu after doubles turn; phase={:?} menu={:?} queue={} finished={}",
        s.battle().phase(),
        s.menu(),
        s.queued().len(),
        s.is_finished()
    );
}

#[test]
fn a_plugin_fx_shows_on_the_opponent_until_the_plugin_clears_it() {
    use pkrs_core::Pcg64Rng;
    use pkrs_script::{LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner};

    let mut host = LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(7)),
        ScriptLimits::default(),
    )
    .unwrap();
    host.run_source(
        &ScriptOwner::plugin("glow"),
        "plugins/glow/init.lua",
        "battle.on('on_pre_turn', function(ctx)
           if ctx.turn == 1 then ctx.play_fx('glow:aura', ctx.creature('opponent', 0)) end
         end)
         battle.on('on_end_of_turn', function(ctx)
           if ctx.turn == 2 then ctx.clear_fx(ctx.creature('opponent', 0)) end
         end)",
    )
    .unwrap();
    let mut setup = battle_testkit::wild_1v1();
    setup.opponent.party[0].stats.max_hp = 500;
    setup.player.party[0].stats.max_hp = 500;
    let mut hooks = host.battle_hooks();
    let mut s = screen_with(rival(), setup, BTreeMap::new(), &mut hooks);
    let mut input = input();
    read_texts(&mut s, &mut input, &mut hooks);
    assert_eq!(s.slot_fx(SideId::Opponent, 0), None);

    let mut fight_turn = |s: &mut BattleScreen| {
        press(s, &mut input, Key::Enter, &mut hooks); // FIGHT
        press(s, &mut input, Key::Enter, &mut hooks); // Tackle
        read_texts(s, &mut input, &mut hooks);
        assert_eq!(s.menu(), Some(MenuKind::Action));
    };
    fight_turn(&mut s);
    assert_eq!(s.slot_fx(SideId::Opponent, 0), Some("glow:aura"));
    assert_eq!(
        s.slot_fx(SideId::Player, 0),
        None,
        "only the opponent glows"
    );
    fight_turn(&mut s);
    assert_eq!(
        s.slot_fx(SideId::Opponent, 0),
        None,
        "the plugin cleared it"
    );
}
