//! Player and AI action input (§7.2).

use crate::model::{PartyIndex, SideId, SlotIndex};

/// What a side chooses for one active slot during [`SelectActions`](crate::phase::BattlePhase::SelectActions)
/// or a forced switch during [`Replace`](crate::phase::BattlePhase::Replace).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleAction {
    /// Use a move on a target slot on the opposing side.
    Fight {
        /// Move index `0..4` on the acting creature.
        move_slot: u8,
        /// Target side.
        target_side: SideId,
        /// Target slot on that side.
        target_slot: SlotIndex,
    },
    /// Switch the active creature to a party index.
    Switch {
        /// Party index to send out.
        to_party: PartyIndex,
    },
    /// Use an item (stub — emits a message only in this milestone).
    Item {
        /// Item content id.
        item_id: String,
        /// Target side.
        target_side: SideId,
        /// Target slot on that side.
        target_slot: SlotIndex,
    },
    /// Attempt to flee (wild battles; trainers may reject in a later issue).
    Run,
    /// Plugin-defined action.
    Custom {
        /// Action id.
        id: String,
        /// Opaque payload bytes.
        payload: Vec<u8>,
    },
}

/// One side's choices for the current input step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideInput {
    /// Choices per slot, length must match `side_size`.
    pub slots: Vec<BattleAction>,
}

/// Input bundle for [`Battle::step`](crate::Battle::step).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BattleInput {
    /// Player choices (required in `SelectActions` / partial in `Replace`).
    pub player: Option<SideInput>,
    /// Opponent choices.
    pub opponent: Option<SideInput>,
}

impl BattleInput {
    /// Both sides choose one action per slot (singles or doubles).
    pub fn both(player: SideInput, opponent: SideInput) -> Self {
        Self {
            player: Some(player),
            opponent: Some(opponent),
        }
    }

    /// A single forced switch for one side (replace phase).
    pub fn switch(side: SideId, to_party: PartyIndex) -> Self {
        let action = SideInput {
            slots: vec![BattleAction::Switch { to_party }],
        };
        match side {
            SideId::Player => Self {
                player: Some(action),
                opponent: None,
            },
            SideId::Opponent => Self {
                player: None,
                opponent: Some(action),
            },
        }
    }
}
