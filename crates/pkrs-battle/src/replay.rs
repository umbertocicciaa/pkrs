//! Seeded battle replay helpers (§7.2, §15.2).

use crate::{
    Battle, BattleConfig, BattleEvent, BattleHooks, BattleInput, BattlePhase, BattleResult,
    BattleSetup, NullBattleHooks, Result, RngPolicy,
};

/// Fingerprint of battle config for replay fixtures (`side_size`, hook tracing, …).
#[must_use]
pub fn config_hash(config: BattleConfig) -> u64 {
    u64::from(config.side_size)
        | (u64::from(config.trace_hooks as u8) << 8)
        | (u64::from(config.side_size) << 16)
}

/// Recorded battle inputs plus the policy and config needed to reproduce the event log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleReplayRecord {
    /// RNG policy used for the original run.
    pub policy: RngPolicy,
    /// Hash of [`BattleConfig`] (see [`config_hash`]).
    pub config_hash: u64,
    /// Ordered `step()` inputs from start through end.
    pub inputs: Vec<BattleInput>,
}

impl BattleReplayRecord {
    /// Replays `inputs` and returns the concatenated event log and final result.
    pub fn replay(
        &self,
        setup: &BattleSetup,
        config: BattleConfig,
        resources: crate::BattleResources,
        hooks: &mut dyn BattleHooks,
    ) -> Result<(Vec<BattleEvent>, BattleResult)> {
        assert_eq!(
            config_hash(config),
            self.config_hash,
            "config hash mismatch"
        );
        let mut battle = Battle::new(setup.clone(), config, resources, self.policy, hooks)?;
        let mut events = Vec::new();
        for input in &self.inputs {
            events.extend(battle.step(input.clone(), hooks)?);
        }
        assert_eq!(battle.phase(), BattlePhase::End, "replay must finish");
        Ok((
            events,
            *battle.result().ok_or_else(|| {
                crate::BattleError::InvalidAction("battle ended without result".into())
            })?,
        ))
    }
}

/// Runs a battle, recording every input, for snapshot/replay tests.
pub fn record_battle(
    setup: BattleSetup,
    config: BattleConfig,
    resources: crate::BattleResources,
    policy: RngPolicy,
    mut produce_input: impl FnMut(&Battle) -> BattleInput,
    hooks: &mut dyn BattleHooks,
) -> Result<(BattleReplayRecord, Vec<BattleEvent>, BattleResult)> {
    let config_hash = config_hash(config);
    let mut battle = Battle::new(setup, config, resources, policy, hooks)?;
    let mut inputs = Vec::new();
    let mut events = Vec::new();
    for _ in 0..10_000 {
        if battle.phase() == BattlePhase::End {
            break;
        }
        let input = produce_input(&battle);
        inputs.push(input.clone());
        events.extend(battle.step(input, hooks)?);
    }
    assert_eq!(
        battle.phase(),
        BattlePhase::End,
        "recorded battle must finish"
    );
    let record = BattleReplayRecord {
        policy,
        config_hash,
        inputs,
    };
    Ok((
        record,
        events,
        *battle.result().ok_or_else(|| {
            crate::BattleError::InvalidAction("battle ended without result".into())
        })?,
    ))
}

/// Convenience replay with default null hooks.
pub fn replay_with_null_hooks(
    record: &BattleReplayRecord,
    setup: &BattleSetup,
    config: BattleConfig,
    resources: crate::BattleResources,
) -> Result<(Vec<BattleEvent>, BattleResult)> {
    let mut hooks = NullBattleHooks;
    record.replay(setup, config, resources, &mut hooks)
}
