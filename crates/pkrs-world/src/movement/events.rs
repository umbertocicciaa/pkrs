//! Events emitted when a step completes (§6.3).

use pkrs_core::{Direction, GridPos};

/// Movement events for scripts and encounter rolls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepEvent {
    /// Facing changed without moving (tap-turn).
    Turned {
        /// Tile after the turn.
        pos: GridPos,
        /// New facing.
        facing: Direction,
    },
    /// A step animation finished on a new tile.
    StepCompleted {
        /// Tile after the step.
        pos: GridPos,
        /// Facing after the step.
        facing: Direction,
        /// Alternating walk cycle foot (`0` or `1`).
        foot: u8,
    },
    /// A step left the map across a seamless edge (§6.4).
    CrossedMap {
        /// Map being entered.
        to_map: String,
        /// Tile on `to_map` after the crossing.
        entry: GridPos,
        /// Last tile on the previous map (may be out of bounds).
        exit: GridPos,
    },
}
