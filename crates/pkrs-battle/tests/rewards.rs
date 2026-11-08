//! Default battle rewards and `on_battle_end` (§7.4): a player win starts the result with the
//! rewards set on the battle, hooks may change them, other outcomes start with none.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleHookCtx, BattleHooks, BattleInput, BattleOutcome,
    BattlePhase, BattleResult, BattleRewards, NullBattleHooks, RngPolicy, SideId, SideInput,
    battle_testkit,
};

fn tackle_turn() -> BattleInput {
    let fight = |side| BattleAction::Fight {
        move_slot: 0,
        target_side: side,
        target_slot: 0,
    };
    BattleInput::both(
        SideInput {
            slots: vec![fight(SideId::Opponent)],
        },
        SideInput {
            slots: vec![fight(SideId::Player)],
        },
    )
}

fn run(
    setup: pkrs_battle::BattleSetup,
    rewards: Option<BattleRewards>,
    hooks: &mut dyn BattleHooks,
) -> BattleResult {
    let mut battle = Battle::new(
        setup,
        BattleConfig::default(),
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        hooks,
    )
    .unwrap();
    if let Some(rewards) = rewards {
        battle.set_rewards(rewards);
        assert_eq!(battle.rewards(), rewards);
    }
    battle.step(BattleInput::default(), hooks).unwrap();
    for _ in 0..100 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        battle.step(tackle_turn(), hooks).unwrap();
    }
    battle.result().copied().unwrap()
}

const REWARDS: BattleRewards = BattleRewards {
    exp: 120,
    money: 40,
};

#[test]
fn a_win_starts_with_the_default_rewards() {
    let result = run(
        battle_testkit::wild_1v1(),
        Some(REWARDS),
        &mut NullBattleHooks,
    );
    assert_eq!(result.outcome, BattleOutcome::PlayerWin);
    assert_eq!((result.exp, result.money), (120, 40));
    let unset = run(battle_testkit::wild_1v1(), None, &mut NullBattleHooks);
    assert_eq!((unset.exp, unset.money), (0, 0));
}

#[test]
fn a_loss_has_no_rewards() {
    let mut setup = battle_testkit::wild_1v1();
    std::mem::swap(&mut setup.player.party, &mut setup.opponent.party);
    let result = run(setup, Some(REWARDS), &mut NullBattleHooks);
    assert_eq!(result.outcome, BattleOutcome::OpponentWin);
    assert_eq!((result.exp, result.money), (0, 0));
}

struct DoubleExp {
    seen: Option<u32>,
}

impl BattleHooks for DoubleExp {
    fn on_battle_end(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        result: &mut BattleResult,
    ) -> pkrs_battle::Result<()> {
        self.seen = Some(result.exp);
        result.exp *= 2;
        Ok(())
    }
}

#[test]
fn on_battle_end_sees_and_changes_the_default_exp() {
    let mut hooks = DoubleExp { seen: None };
    let result = run(battle_testkit::wild_1v1(), Some(REWARDS), &mut hooks);
    assert_eq!(hooks.seen, Some(120));
    assert_eq!((result.exp, result.money), (240, 40));
}
