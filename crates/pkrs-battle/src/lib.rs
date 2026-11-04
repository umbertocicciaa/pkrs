//! Pure, deterministic battle logic (§7). No rendering, scripting or I/O.
//!
//! See `docs/design.md` §7.1–§7.2 and §4.3 for crate boundaries.

mod action;
mod battle;
mod convert;
mod error;
mod event;
mod hooks;
mod model;
mod order;
mod phase;
mod rng;

pub use action::{BattleAction, BattleInput, SideInput};
pub use battle::Battle;
pub use convert::{IntoBattleSetup, testkit as battle_testkit};
pub use error::{BattleError, Result};
pub use event::BattleEvent;
pub use hooks::{ActionOrder, BattleHookCtx, BattleHooks, NullBattleHooks, RecordingBattleHooks};
pub use model::{
    BattleConfig, BattleCreature, BattleOutcome, BattleResult, BattleSetup, BattleSideSetup,
    CreatureStats, MoveSlot, PartyIndex, SideId, SlotIndex,
};
pub use phase::BattlePhase;
pub use rng::{BattleRng, RngPolicy};

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
