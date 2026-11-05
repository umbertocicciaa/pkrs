//! Pure, deterministic battle logic (§7). No rendering, scripting or I/O.
//!
//! See `docs/design.md` §7.1–§7.2 and §4.3 for crate boundaries.

mod accuracy;
mod action;
mod ai;
mod battle;
mod convert;
mod damage;
mod effects;
mod error;
mod event;
mod hooks;
mod model;
mod order;
mod phase;
mod resources;
mod rng;
mod stat_stage;

pub use accuracy::AccuracyCalc;
pub use action::{BattleAction, BattleInput, SideInput};
pub use ai::{AiView, BattleAi, TrainerAi, ai_side_input};
pub use battle::Battle;
pub use convert::{IntoBattleSetup, testkit as battle_testkit};
pub use damage::{DamageCalc, DamageModifierStage};
pub use error::{BattleError, Result};
pub use event::BattleEvent;
pub use hooks::{
    ActionMenu, ActionOrder, BattleHookCtx, BattleHooks, BattleValue, BattleView, CreatureView,
    CurrentMove, CustomAction, DamageTaken, FaintEvent, HookResult, MenuOption, MoveRequest,
    MoveResult, MoveView, NullBattleHooks, RecordingBattleHooks, SideView, SlotRef, StatusApply,
    SwitchIn, SwitchOut,
};
pub use model::{
    BattleConfig, BattleCreature, BattleOutcome, BattleResult, BattleSetup, BattleSideSetup,
    CreatureStats, MoveSlot, PartyIndex, SideId, SlotIndex,
};
pub use phase::BattlePhase;
pub use resources::BattleResources;
pub use rng::{BattleRng, RngPolicy};
pub use stat_stage::StatStages;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: BattleError = BattleError::InvalidAction("no PP left".into());
        assert_eq!(err.to_string(), "invalid action: no PP left");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
