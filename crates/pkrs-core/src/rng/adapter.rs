//! `rand_pcg::Pcg64` adapter — the only module that imports `rand_pcg`.

use rand_core::{RngCore, SeedableRng};
use rand_pcg::Pcg64;

use super::{GameRng, RngState};

/// PCG64-backed game RNG (§4.5).
#[derive(Clone, Debug)]
pub struct Pcg64Rng {
    root_seed: u64,
    steps: u64,
    inner: Pcg64,
}

impl Pcg64Rng {
    /// Creates an RNG from a 64-bit root seed (new game).
    pub fn from_seed(seed: u64) -> Self {
        Self {
            root_seed: seed,
            steps: 0,
            inner: Pcg64::seed_from_u64(seed),
        }
    }

    fn draw_u32(&mut self) -> u32 {
        self.steps = self.steps.saturating_add(1);
        self.inner.next_u32()
    }
}

impl GameRng for Pcg64Rng {
    fn next_u32(&mut self) -> u32 {
        self.draw_u32()
    }

    fn derive_sub_seed(&mut self, _label: u64) -> u64 {
        self.next_u64()
    }

    fn state(&self) -> RngState {
        RngState {
            seed: self.root_seed,
            steps: self.steps,
        }
    }

    fn restore_state(&mut self, state: RngState) {
        *self = Self::from_seed(state.seed);
        if state.steps > 0 {
            self.inner.advance(u128::from(state.steps));
        }
        self.steps = state.steps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::GameRng;

    #[test]
    fn rng_state_serializes() {
        let rng = Pcg64Rng::from_seed(42);
        let state = rng.state();
        let json = serde_json::to_string(&state).expect("serialize");
        let back: RngState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state, back);
    }
}
