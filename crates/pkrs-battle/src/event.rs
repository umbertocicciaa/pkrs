//! Append-only battle log entries (§7.2).

use pkrs_data::StatId;

use crate::action::BattleAction;
use crate::model::{BattleOutcome, PartyIndex, SideId, SlotIndex};

/// One record in the battle event log (UI / replay consumes these in order).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BattleEvent {
    /// The battle left setup and entered turn 1.
    BattleStarted,
    /// A new turn begins (`turn` is 1-based).
    TurnStarted {
        /// Turn number.
        turn: u32,
    },
    /// Both sides locked in their actions for this turn.
    ActionsLocked,
    /// Speed / priority order was computed for this turn.
    OrderResolved {
        /// `(side, slot)` in execution order.
        order: Vec<(SideId, SlotIndex)>,
    },
    /// A side committed to an action (debug / replay).
    ActionCommitted {
        /// Acting side.
        side: SideId,
        /// Acting slot.
        slot: SlotIndex,
        /// What they chose.
        action: BattleAction,
    },
    /// A move was used (damage may follow in separate events).
    MoveUsed {
        /// User side.
        user_side: SideId,
        /// User slot.
        user_slot: SlotIndex,
        /// Move content id.
        move_id: String,
        /// Target side.
        target_side: SideId,
        /// Target slot.
        target_slot: SlotIndex,
    },
    /// Damage dealt after the full pipeline.
    Damage {
        /// Target side.
        target_side: SideId,
        /// Target slot.
        target_slot: SlotIndex,
        /// Hit points removed.
        amount: u32,
        /// HP remaining on the target.
        remaining_hp: u32,
    },
    /// A move missed.
    Missed {
        /// Target side.
        target_side: SideId,
        /// Target slot.
        target_slot: SlotIndex,
        /// Move content id.
        move_id: String,
    },
    /// A stat stage changed.
    StatChanged {
        /// Affected side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
        /// Which stat.
        stat: StatId,
        /// New stage (-6..=6).
        stages: i8,
    },
    /// A status was applied.
    StatusApplied {
        /// Target side.
        target_side: SideId,
        /// Target slot.
        target_slot: SlotIndex,
        /// Status id.
        status: String,
    },
    /// Heal (absolute amount or percent placeholder in `amount`).
    Heal {
        /// Side healed.
        side: SideId,
        /// Slot healed.
        slot: SlotIndex,
        /// Heal amount (semantics depend on effect).
        amount: u32,
    },
    /// Recoil damage to the user.
    Recoil {
        /// User side.
        side: SideId,
        /// User slot.
        slot: SlotIndex,
        /// Damage dealt to self.
        amount: u32,
    },
    /// Field weather changed.
    WeatherSet {
        /// Weather id.
        weather: String,
    },
    /// A creature fainted.
    Fainted {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
        /// Party index of the fainted creature.
        party_index: PartyIndex,
    },
    /// A creature was switched in.
    Switched {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
        /// Party index sent out.
        to_party: PartyIndex,
    },
    /// A side ran from battle.
    Ran {
        /// Who ran.
        side: SideId,
    },
    /// Informational message key (localisation happens in UI).
    Message {
        /// Message key.
        key: String,
    },
    /// End-of-turn marker (residual effects land in later issues).
    TurnEnded {
        /// Turn that ended.
        turn: u32,
    },
    /// The battle is over.
    BattleEnded {
        /// Final outcome.
        outcome: BattleOutcome,
    },
}
