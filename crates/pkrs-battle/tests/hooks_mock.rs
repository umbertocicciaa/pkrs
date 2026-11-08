//! Mock `BattleHooks` tests for each hook point and its cancel semantics (§7.4).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    AccuracyCalc, ActionMenu, ActionOrder, Battle, BattleAction, BattleConfig, BattleCreature,
    BattleEvent, BattleHookCtx, BattleHooks, BattleInput, BattlePhase, BattleResources,
    BattleResult, BattleSetup, BattleSideSetup, BattleValue, CreatureStats, CustomAction,
    DamageCalc, DamageTaken, FaintEvent, HookResult, MenuOption, MoveRequest, MoveResult, MoveSlot,
    NullBattleHooks, Result, RngPolicy, SideId, SideInput, SlotRef, StatusApply, SwitchIn,
    SwitchOut,
};
use pkrs_data::{BattlePack, MoveCategory, MoveDef, MoveEffect, TypeChart};

const P: SlotRef = SlotRef::new(SideId::Player, 0);
const O: SlotRef = SlotRef::new(SideId::Opponent, 0);

fn resources() -> BattleResources {
    let chart = TypeChart::from_parts(vec!["normal".into()], std::collections::HashMap::new());
    let mut pack = BattlePack::new(chart);
    for def in [
        MoveDef {
            id: "tackle".into(),
            type_id: "normal".into(),
            category: MoveCategory::Physical,
            power: 40,
            accuracy: 100,
            priority: 0,
            targeting: Default::default(),
            effects: vec![MoveEffect::Damage],
        },
        MoveDef {
            id: "spore".into(),
            type_id: "normal".into(),
            category: MoveCategory::Status,
            power: 0,
            accuracy: 0,
            priority: 0,
            targeting: Default::default(),
            effects: vec![MoveEffect::ApplyStatus {
                status: "sleep".into(),
                chance: 100,
            }],
        },
    ] {
        pack.insert_move(def).unwrap();
    }
    BattleResources::new(pack)
}

fn mv(id: &str) -> Option<MoveSlot> {
    Some(MoveSlot {
        id: id.into(),
        priority: 0,
        power: 40,
        pp: 10,
        max_pp: 10,
    })
}

fn creature(name: &str, hp: u32, speed: u32) -> BattleCreature {
    BattleCreature {
        species_id: name.into(),
        types: vec!["normal".into()],
        nickname: name.into(),
        level: 10,
        stats: CreatureStats::new(hp, 20, 10, speed),
        moves: [mv("tackle"), mv("spore"), None, None],
    }
}

fn setup() -> BattleSetup {
    BattleSetup {
        player: BattleSideSetup {
            name: "P".into(),
            party: vec![creature("hero", 100, 30), creature("backup", 100, 5)],
            leading: vec![Some(0)],
        },
        opponent: BattleSideSetup {
            name: "O".into(),
            party: vec![creature("foe", 100, 10), creature("foe2", 100, 1)],
            leading: vec![Some(0)],
        },
    }
}

fn battle_with(config: BattleConfig, hooks: &mut dyn BattleHooks) -> (Battle, Vec<BattleEvent>) {
    let mut battle = Battle::new(
        setup(),
        config,
        resources(),
        RngPolicy::Fixed { seed: 3 },
        hooks,
    )
    .unwrap();
    let events = battle.step(BattleInput::default(), hooks).unwrap();
    assert_eq!(battle.phase(), BattlePhase::SelectActions);
    (battle, events)
}

fn start(hooks: &mut dyn BattleHooks) -> Battle {
    battle_with(BattleConfig::default(), hooks).0
}

fn fight(slot: u8, target: SideId) -> BattleAction {
    BattleAction::Fight {
        move_slot: slot,
        target_side: target,
        target_slot: 0,
    }
}

fn turn(player: BattleAction, opponent: BattleAction) -> BattleInput {
    BattleInput::both(
        SideInput {
            slots: vec![player],
        },
        SideInput {
            slots: vec![opponent],
        },
    )
}

fn tackle_turn() -> BattleInput {
    turn(fight(0, SideId::Opponent), fight(0, SideId::Player))
}

fn damage_to(events: &[BattleEvent], side: SideId) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            BattleEvent::Damage {
                target_side,
                amount,
                ..
            } if *target_side == side => Some(*amount),
            _ => None,
        })
        .collect()
}

#[test]
fn damage_calc_hook_can_push_modifiers_and_override() {
    struct Double;
    impl BattleHooks for Double {
        fn on_damage_calc(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            dmg: &mut DamageCalc,
        ) -> Result<()> {
            dmg.push_modifier("double", 2.0);
            Ok(())
        }
    }
    struct Fixed;
    impl BattleHooks for Fixed {
        fn on_damage_calc(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            dmg: &mut DamageCalc,
        ) -> Result<()> {
            dmg.set_final(7);
            Ok(())
        }
    }
    let base = {
        let mut h = NullBattleHooks;
        let mut b = start(&mut h);
        damage_to(&b.step(tackle_turn(), &mut h).unwrap(), SideId::Opponent)
    };
    let doubled = {
        let mut h = Double;
        let mut b = start(&mut h);
        damage_to(&b.step(tackle_turn(), &mut h).unwrap(), SideId::Opponent)
    };
    assert_eq!(doubled[0], base[0] * 2, "same seed, doubled damage");
    let mut h = Fixed;
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert_eq!(damage_to(&events, SideId::Opponent), [7]);
    assert_eq!(damage_to(&events, SideId::Player), [7]);
}

#[test]
fn before_move_can_cancel_a_move() {
    struct Flinch;
    impl BattleHooks for Flinch {
        fn on_before_move(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            req: &mut MoveRequest,
        ) -> Result<HookResult> {
            Ok(if req.user.side == SideId::Opponent {
                HookResult::cancel("text:battle.flinch")
            } else {
                HookResult::Continue
            })
        }
    }
    let mut h = Flinch;
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert!(events.contains(&BattleEvent::MoveCancelled {
        user_side: SideId::Opponent,
        user_slot: 0,
        move_id: "tackle".into(),
        reason: Some("text:battle.flinch".into()),
    }));
    assert_eq!(damage_to(&events, SideId::Player), Vec::<u32>::new());
    assert_eq!(damage_to(&events, SideId::Opponent).len(), 1);
}

#[test]
fn before_move_can_swap_the_move_and_bad_targets_fall_back() {
    struct Swap;
    impl BattleHooks for Swap {
        fn on_before_move(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            req: &mut MoveRequest,
        ) -> Result<HookResult> {
            if req.user.side == SideId::Player {
                req.move_slot = 1; // spore
            } else {
                req.target = SlotRef::new(SideId::Player, 5); // no such slot: ignored
            }
            Ok(HookResult::Continue)
        }
    }
    let mut h = Swap;
    let mut s = setup();
    // Opponent must move before player's spore, or sleep would block the tackle.
    s.player.party[0] = creature("hero", 100, 10);
    s.opponent.party[0] = creature("foe", 100, 50);
    let mut b = Battle::new(
        s,
        BattleConfig::default(),
        resources(),
        RngPolicy::Fixed { seed: 3 },
        &mut h,
    )
    .unwrap();
    b.step(BattleInput::default(), &mut h).unwrap();
    assert_eq!(b.phase(), BattlePhase::SelectActions);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert!(events.iter().any(|e| matches!(e, BattleEvent::MoveUsed { user_side: SideId::Player, move_id, .. } if move_id == "spore")));
    assert_eq!(
        damage_to(&events, SideId::Player).len(),
        1,
        "opponent kept its target"
    );
}

#[test]
fn accuracy_hook_can_force_a_miss_and_after_move_sees_it() {
    #[derive(Default)]
    struct Miss {
        results: Vec<MoveResult>,
    }
    impl BattleHooks for Miss {
        fn on_accuracy(&mut self, _: &mut BattleHookCtx<'_>, acc: &mut AccuracyCalc) -> Result<()> {
            acc.hits = false;
            Ok(())
        }
        fn on_after_move(&mut self, _: &mut BattleHookCtx<'_>, res: &MoveResult) -> Result<()> {
            self.results.push(res.clone());
            Ok(())
        }
    }
    let mut h = Miss::default();
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, BattleEvent::Missed { .. }))
            .count(),
        2
    );
    assert_eq!(h.results.len(), 2);
    assert!(h.results.iter().all(|r| !r.hit && r.damage == 0));
}

#[test]
fn damage_taken_can_change_or_cancel_damage() {
    struct Shield;
    impl BattleHooks for Shield {
        fn on_damage_taken(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            hit: &mut DamageTaken,
        ) -> Result<HookResult> {
            if hit.target == P {
                return Ok(HookResult::cancel("text:shield"));
            }
            hit.amount = 1;
            Ok(HookResult::Continue)
        }
    }
    let mut h = Shield;
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert_eq!(damage_to(&events, SideId::Opponent), [1]);
    assert_eq!(damage_to(&events, SideId::Player), Vec::<u32>::new());
    assert!(events.contains(&BattleEvent::DamagePrevented {
        target_side: SideId::Player,
        target_slot: 0,
        reason: Some("text:shield".into()),
    }));
}

#[test]
fn status_apply_can_change_or_block_the_status() {
    struct Immune;
    impl BattleHooks for Immune {
        fn on_status_apply(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            req: &mut StatusApply,
        ) -> Result<HookResult> {
            if req.target == P {
                return Ok(HookResult::cancel("text:immune"));
            }
            req.status = "drowsy".into();
            Ok(HookResult::Continue)
        }
    }
    let mut h = Immune;
    let mut b = start(&mut h);
    let events = b
        .step(
            turn(fight(1, SideId::Opponent), fight(1, SideId::Player)),
            &mut h,
        )
        .unwrap();
    assert!(events.contains(&BattleEvent::StatusApplied {
        target_side: SideId::Opponent,
        target_slot: 0,
        status: "drowsy".into(),
    }));
    assert!(events.contains(&BattleEvent::StatusBlocked {
        target_side: SideId::Player,
        target_slot: 0,
        status: "sleep".into(),
        reason: Some("text:immune".into()),
    }));
}

#[derive(Default)]
struct Trap {
    seen_out: Vec<SwitchOut>,
    seen_in: Vec<SwitchIn>,
    trap: bool,
}

impl BattleHooks for Trap {
    fn on_switch_out(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        sw: &mut SwitchOut,
    ) -> Result<HookResult> {
        self.seen_out.push(sw.clone());
        Ok(if self.trap {
            HookResult::cancel("text:trapped")
        } else {
            HookResult::Continue
        })
    }
    fn on_switch_in(&mut self, _: &mut BattleHookCtx<'_>, sw: &mut SwitchIn) -> Result<HookResult> {
        self.seen_in.push(sw.clone());
        Ok(HookResult::Continue)
    }
}

#[test]
fn switch_out_can_trap_a_voluntary_switch() {
    let mut h = Trap {
        trap: true,
        ..Trap::default()
    };
    let mut b = start(&mut h);
    let events = b
        .step(
            turn(
                BattleAction::Switch { to_party: 1 },
                fight(0, SideId::Player),
            ),
            &mut h,
        )
        .unwrap();
    assert!(events.contains(&BattleEvent::SwitchCancelled {
        side: SideId::Player,
        slot: 0,
        reason: Some("text:trapped".into()),
    }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, BattleEvent::Switched { .. }))
    );
    assert_eq!(b.view().side(SideId::Player).active, [Some(0)]);
    assert!(
        h.seen_in.is_empty(),
        "switch-in hook not reached once trapped"
    );

    let mut h = Trap::default();
    let mut b = start(&mut h);
    let events = b
        .step(
            turn(
                BattleAction::Switch { to_party: 1 },
                fight(0, SideId::Player),
            ),
            &mut h,
        )
        .unwrap();
    assert!(events.contains(&BattleEvent::Switched {
        side: SideId::Player,
        slot: 0,
        to_party: 1
    }));
    assert_eq!(h.seen_out[0].party_index, Some(0));
    assert_eq!(h.seen_out[0].replacement, 1);
    assert!(!h.seen_in[0].forced);
    // The switch goes first (priority), so the foe's tackle hits the backup.
    assert_eq!(b.view().side(SideId::Player).active, [Some(1)]);
}

#[test]
fn forced_replacement_ignores_cancel_and_faint_hook_fires() {
    #[derive(Default)]
    struct Ko {
        trap: Trap,
        faints: Vec<FaintEvent>,
    }
    impl BattleHooks for Ko {
        fn on_damage_calc(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            dmg: &mut DamageCalc,
        ) -> Result<()> {
            dmg.set_final(1000);
            Ok(())
        }
        fn on_faint(&mut self, _: &mut BattleHookCtx<'_>, f: &FaintEvent) -> Result<()> {
            self.faints.push(f.clone());
            Ok(())
        }
        fn on_switch_out(
            &mut self,
            c: &mut BattleHookCtx<'_>,
            s: &mut SwitchOut,
        ) -> Result<HookResult> {
            self.trap.on_switch_out(c, s)
        }
        fn on_switch_in(
            &mut self,
            c: &mut BattleHookCtx<'_>,
            s: &mut SwitchIn,
        ) -> Result<HookResult> {
            self.trap.on_switch_in(c, s)
        }
    }
    let mut h = Ko {
        trap: Trap {
            trap: true,
            ..Trap::default()
        },
        ..Ko::default()
    };
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    // Player is faster: the foe faints and its queued tackle is dropped.
    assert_eq!(
        h.faints,
        [FaintEvent {
            slot: O,
            party_index: 0
        }]
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        BattleEvent::ActionCommitted {
            side: SideId::Opponent,
            ..
        }
    )));
    assert_eq!(
        b.phase(),
        BattlePhase::Replace {
            side: SideId::Opponent,
            slot: 0
        }
    );
    let events = b
        .step(BattleInput::switch(SideId::Opponent, 1), &mut h)
        .unwrap();
    assert!(events.contains(&BattleEvent::Switched {
        side: SideId::Opponent,
        slot: 0,
        to_party: 1
    }));
    assert!(h.trap.seen_out.last().unwrap().forced);
}

#[test]
fn battle_end_hook_sets_rewards() {
    struct Rewards;
    impl BattleHooks for Rewards {
        fn on_battle_end(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            result: &mut BattleResult,
        ) -> Result<()> {
            result.money = 120;
            result.exp = 45;
            Ok(())
        }
    }
    let mut h = Rewards;
    let mut b = start(&mut h);
    b.step(turn(BattleAction::Run, fight(0, SideId::Player)), &mut h)
        .unwrap();
    let result = b.result().unwrap();
    assert_eq!((result.money, result.exp), (120, 45));
}

#[test]
fn action_order_hook_can_reorder() {
    struct Reverse;
    impl BattleHooks for Reverse {
        fn on_action_order(
            &mut self,
            _: &mut BattleHookCtx<'_>,
            order: &mut ActionOrder,
        ) -> Result<()> {
            order.entries.reverse();
            Ok(())
        }
    }
    let mut h = Reverse;
    let mut b = start(&mut h);
    let events = b.step(tackle_turn(), &mut h).unwrap();
    assert!(events.contains(&BattleEvent::OrderResolved {
        order: vec![(SideId::Opponent, 0), (SideId::Player, 0)]
    }));
}

/// The Resonance shape from §9.5, as a Rust mock.
struct Resonance;

impl BattleHooks for Resonance {
    fn on_action_menu(&mut self, ctx: &mut BattleHookCtx<'_>, menu: &mut ActionMenu) -> Result<()> {
        if menu.slot.side == SideId::Player
            && ctx.side_state(SideId::Player, "resonance_used").is_none()
        {
            menu.options.push(MenuOption {
                id: "resonance:resonate".into(),
                label: "text:resonance.menu".into(),
            });
        }
        Ok(())
    }

    fn on_custom_action(
        &mut self,
        ctx: &mut BattleHookCtx<'_>,
        action: &mut CustomAction,
    ) -> Result<()> {
        if action.id != "resonance:resonate" {
            return Ok(());
        }
        action.handled = true;
        ctx.set_side_state(action.actor.side, "resonance_used", BattleValue::Bool(true));
        let timer = [("turns".to_string(), BattleValue::Int(3))]
            .into_iter()
            .collect();
        ctx.set_volatile(action.actor, "resonating", BattleValue::Map(timer));
        assert!(
            ctx.volatile(action.actor, "resonating").is_some(),
            "pending writes are visible"
        );
        ctx.message("text:resonance.start");
        Ok(())
    }

    fn on_damage_calc(&mut self, ctx: &mut BattleHookCtx<'_>, dmg: &mut DamageCalc) -> Result<()> {
        if ctx.volatile(P, "resonating").is_some() {
            dmg.push_modifier("resonance", 1.5);
        }
        Ok(())
    }
}

#[test]
fn menu_entry_and_custom_action_round_trip() {
    let mut h = Resonance;
    let mut b = start(&mut h);
    let menu = b.action_menu(P, &mut h).unwrap();
    let ids: Vec<_> = menu.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["fight", "bag", "party", "run", "resonance:resonate"]);

    let custom = BattleAction::Custom {
        id: "resonance:resonate".into(),
        payload: vec![],
    };
    let events = b
        .step(turn(custom, fight(0, SideId::Player)), &mut h)
        .unwrap();
    let resonance_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                BattleEvent::SideStateChanged { .. }
                    | BattleEvent::VolatileChanged { .. }
                    | BattleEvent::Message { .. }
                    | BattleEvent::CustomActionUsed { .. }
            )
        })
        .cloned()
        .collect();
    assert_eq!(
        resonance_events,
        [
            BattleEvent::SideStateChanged {
                side: SideId::Player,
                key: "resonance_used".into(),
                value: Some(BattleValue::Bool(true)),
            },
            BattleEvent::VolatileChanged {
                side: SideId::Player,
                slot: 0,
                key: "resonating".into(),
                value: Some(BattleValue::Map(
                    [("turns".to_string(), BattleValue::Int(3))]
                        .into_iter()
                        .collect()
                )),
            },
            BattleEvent::Message {
                key: "text:resonance.start".into()
            },
            BattleEvent::CustomActionUsed {
                side: SideId::Player,
                slot: 0,
                id: "resonance:resonate".into(),
                handled: true,
            },
        ]
    );
    // The menu entry is gone once used, and the volatile boosts damage next turn.
    let menu = b.action_menu(P, &mut h).unwrap();
    assert!(!menu.contains("resonance:resonate"));
    let view = b.view();
    assert!(
        view.creature(P)
            .unwrap()
            .volatiles
            .contains_key("resonating")
    );
}

#[test]
fn unhandled_custom_action_is_reported() {
    let mut h = NullBattleHooks;
    let mut b = start(&mut h);
    let custom = BattleAction::Custom {
        id: "nobody:home".into(),
        payload: vec![1, 2],
    };
    let events = b
        .step(turn(custom, fight(0, SideId::Player)), &mut h)
        .unwrap();
    assert!(events.contains(&BattleEvent::CustomActionUsed {
        side: SideId::Player,
        slot: 0,
        id: "nobody:home".into(),
        handled: false,
    }));
}

#[test]
fn volatiles_clear_on_switch_out() {
    let mut h = Resonance;
    let mut b = start(&mut h);
    let custom = BattleAction::Custom {
        id: "resonance:resonate".into(),
        payload: vec![],
    };
    b.step(turn(custom, fight(0, SideId::Player)), &mut h)
        .unwrap();
    b.step(
        turn(
            BattleAction::Switch { to_party: 1 },
            fight(0, SideId::Player),
        ),
        &mut h,
    )
    .unwrap();
    let view = b.view();
    assert!(view.creature(P).unwrap().volatiles.is_empty());
    assert_eq!(
        view.side(SideId::Player).state.get("resonance_used"),
        Some(&BattleValue::Bool(true)),
        "side state persists"
    );
}

#[test]
fn ctx_random_is_deterministic_and_part_of_the_stream() {
    #[derive(Default)]
    struct Roller {
        rolls: Vec<u32>,
    }
    impl BattleHooks for Roller {
        fn on_turn_start(&mut self, ctx: &mut BattleHookCtx<'_>) -> Result<()> {
            self.rolls.push(ctx.random(1000));
            Ok(())
        }
    }
    let run = || {
        let mut h = Roller::default();
        let mut b = start(&mut h);
        b.step(tackle_turn(), &mut h).unwrap();
        h.rolls
    };
    let a = run();
    assert_eq!(a, run());
    assert_eq!(a.len(), 2);
}

#[test]
fn trace_hooks_emits_hook_trace_before_each_call() {
    let config = BattleConfig {
        trace_hooks: true,
        ..BattleConfig::default()
    };
    let mut h = NullBattleHooks;
    let (_, events) = battle_with(config, &mut h);
    let traces: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            BattleEvent::HookTrace { hook } => Some(hook.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(traces, ["on_battle_start", "on_turn_start"]);
    assert_eq!(
        events[0],
        BattleEvent::HookTrace {
            hook: "on_battle_start".into()
        }
    );
    assert_eq!(events[1], BattleEvent::BattleStarted);

    let (_, untraced) = battle_with(
        BattleConfig {
            trace_hooks: false,
            ..BattleConfig::default()
        },
        &mut h,
    );
    assert!(
        !untraced
            .iter()
            .any(|e| matches!(e, BattleEvent::HookTrace { .. }))
    );
}

#[test]
fn hook_errors_abort_the_step() {
    struct Broken;
    impl BattleHooks for Broken {
        fn on_pre_turn(&mut self, _: &mut BattleHookCtx<'_>) -> Result<()> {
            Err(pkrs_battle::BattleError::InvalidAction(
                "hook failed".into(),
            ))
        }
    }
    let mut h = Broken;
    let mut b = start(&mut h);
    assert!(b.step(tackle_turn(), &mut h).is_err());
}
