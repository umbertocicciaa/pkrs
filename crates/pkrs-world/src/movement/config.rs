//! Movement timing defaults (§6.3).

/// Tick counts and tap threshold for grid movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementConfig {
    /// Ticks to walk one tile (default 16).
    pub walk_step_ticks: u32,
    /// Ticks to run one tile (default 8).
    pub run_step_ticks: u32,
    /// Direction must be held this many ticks before a step starts (tap shorter = turn only).
    pub tap_turn_ticks: u32,
    /// Ticks for a ledge hop (default 2× walk).
    pub ledge_step_ticks: u32,
}

impl Default for MovementConfig {
    fn default() -> Self {
        Self {
            walk_step_ticks: 16,
            run_step_ticks: 8,
            tap_turn_ticks: 8,
            ledge_step_ticks: 32,
        }
        .sanitized()
    }
}

impl MovementConfig {
    /// Default walking step length in ticks (§6.3).
    pub const WALK_STEP_TICKS: u32 = 16;

    /// Clamps tick counts so interpolation never divides by zero (§6.3).
    #[must_use]
    pub const fn sanitized(self) -> Self {
        Self {
            walk_step_ticks: Self::nonzero(self.walk_step_ticks),
            run_step_ticks: Self::nonzero(self.run_step_ticks),
            tap_turn_ticks: Self::nonzero(self.tap_turn_ticks),
            ledge_step_ticks: Self::nonzero(self.ledge_step_ticks),
        }
    }

    const fn nonzero(n: u32) -> u32 {
        if n == 0 { 1 } else { n }
    }
}
