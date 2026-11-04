//! Turn phase state machine (§7.1).

/// Current phase of the battle state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattlePhase {
    /// Initial hooks; transitions automatically on the next step.
    Start,
    /// Waiting for [`BattleInput`](crate::action::BattleInput) from each side.
    SelectActions,
    /// Before speed calculation (forme / item hooks).
    PreTurn,
    /// Actions sorted; about to execute.
    ResolveOrder,
    /// Executing one queued action at a time.
    ExecuteAction,
    /// Check for faints and forced replacements.
    FaintCheck,
    /// Waiting for a mandatory switch on one side/slot.
    Replace {
        /// Side that must switch.
        side: crate::model::SideId,
        /// Slot that needs a new creature.
        slot: crate::model::SlotIndex,
    },
    /// End-of-turn hooks and second faint check.
    EndOfTurn,
    /// Terminal; [`Battle::result`](crate::Battle::result) is set.
    End,
}
