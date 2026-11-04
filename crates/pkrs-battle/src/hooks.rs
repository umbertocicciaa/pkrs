//! Synchronous battle hooks (§7.4); default implementations are no-ops.

use crate::error::Result;
use crate::event::BattleEvent;
use crate::model::{BattleResult, SideId, SlotIndex};
use crate::phase::BattlePhase;

/// Read-only battle snapshot for hooks (expanded in later issues).
#[derive(Clone, Copy, Debug)]
pub struct BattleHookCtx<'a> {
    /// Current phase.
    pub phase: BattlePhase,
    /// 1-based turn counter.
    pub turn: u32,
    /// Side size (active slots).
    pub side_size: u8,
    /// Event log so far this step (append-only).
    pub events: &'a [BattleEvent],
}

/// Mutable ordering list for [`BattleHooks::on_action_order`].
#[derive(Clone, Debug, Default)]
pub struct ActionOrder {
    /// `(side, slot)` entries to execute.
    pub entries: Vec<(SideId, SlotIndex)>,
}

/// Hook surface invoked by the battle core (Lua / plugins implement via `pkrs-script`).
pub trait BattleHooks {
    /// Called once when leaving [`BattlePhase::Start`].
    fn on_battle_start(&mut self, _ctx: &BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// Called at the beginning of each turn (after `TurnStarted` is queued).
    fn on_turn_start(&mut self, _ctx: &BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// Called in [`BattlePhase::PreTurn`].
    fn on_pre_turn(&mut self, _ctx: &BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// Called after priority/speed sort; may reorder `order.entries`.
    fn on_action_order(
        &mut self,
        _ctx: &BattleHookCtx<'_>,
        _order: &mut ActionOrder,
    ) -> Result<()> {
        Ok(())
    }

    /// Called in [`BattlePhase::EndOfTurn`].
    fn on_end_of_turn(&mut self, _ctx: &BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// Called when the battle reaches [`BattlePhase::End`].
    fn on_battle_end(
        &mut self,
        _ctx: &BattleHookCtx<'_>,
        _result: &mut BattleResult,
    ) -> Result<()> {
        Ok(())
    }
}

/// No-op hooks for tests and headless simulations.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullBattleHooks;

impl BattleHooks for NullBattleHooks {}

/// Records hook names invoked (tests only).
#[derive(Clone, Debug, Default)]
pub struct RecordingBattleHooks {
    /// Hook names in call order.
    pub log: Vec<&'static str>,
}

impl BattleHooks for RecordingBattleHooks {
    fn on_battle_start(&mut self, _: &BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_battle_start");
        Ok(())
    }

    fn on_turn_start(&mut self, _: &BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_turn_start");
        Ok(())
    }

    fn on_pre_turn(&mut self, _: &BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_pre_turn");
        Ok(())
    }

    fn on_action_order(&mut self, _: &BattleHookCtx<'_>, _: &mut ActionOrder) -> Result<()> {
        self.log.push("on_action_order");
        Ok(())
    }

    fn on_battle_end(&mut self, _: &BattleHookCtx<'_>, _: &mut BattleResult) -> Result<()> {
        self.log.push("on_battle_end");
        Ok(())
    }
}
