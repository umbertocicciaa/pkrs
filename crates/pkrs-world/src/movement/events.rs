//! Events emitted when a step completes (§6.3).

use pkrs_core::{Direction, GridPos};

/// Movement events for scripts and encounter rolls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}
