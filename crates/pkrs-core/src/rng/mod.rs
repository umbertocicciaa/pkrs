//! `GameRng` port and the `Pcg64Rng` adapter (§4.5).

mod adapter;

use serde::{Deserialize, Serialize};

pub use adapter::Pcg64Rng;

/// Serializable snapshot of the game RNG stream (stored in saves).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngState {
    /// Root seed chosen at new game (or loaded save).
    pub seed: u64,
    /// Number of `next_u32` draws consumed on the main stream.
    pub steps: u64,
}

/// Engine-owned randomness port (no vendor types in this trait).
pub trait GameRng {
    /// Returns the next 32-bit value.
    fn next_u32(&mut self) -> u32;

    /// Returns the next 64-bit value (default: two `next_u32` halves).
    fn next_u64(&mut self) -> u64 {
        let hi = u64::from(self.next_u32());
        let lo = u64::from(self.next_u32());
        (hi << 32) | lo
    }

    /// Uniform index in `[0, upper)`; returns `0` when `upper == 0`.
    fn gen_range_u32(&mut self, upper: u32) -> u32 {
        if upper == 0 {
            return 0;
        }
        // Rejection sampling to avoid modulo bias.
        let threshold = u32::MAX - u32::MAX % upper;
        loop {
            let sample = self.next_u32();
            if sample < threshold {
                return sample % upper;
            }
        }
    }

    /// Draws a battle/sub-system seed from the main stream (§7.2); advances save state.
    ///
    /// The `label` argument is reserved for future structured tagging; today the seed is
    /// `next_u64()` from the main stream so each battle differs and reloads stay consistent.
    fn derive_sub_seed(&mut self, _label: u64) -> u64;

    /// Captures the main-stream state for saves.
    fn state(&self) -> RngState;

    /// Restores the main-stream state from a save.
    fn restore_state(&mut self, state: RngState);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Pcg64Rng::from_seed(0xDEAD_BEEF);
        let mut b = Pcg64Rng::from_seed(0xDEAD_BEEF);
        for _ in 0..32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn state_round_trip_continues_sequence() {
        let mut rng = Pcg64Rng::from_seed(99);
        let expected: Vec<u32> = (0..20).map(|_| rng.next_u32()).collect();

        let mut mid = Pcg64Rng::from_seed(99);
        for _ in 0..10 {
            let _ = mid.next_u32();
        }
        let snapshot = mid.state();

        let mut restored = Pcg64Rng::from_seed(99);
        restored.restore_state(snapshot);

        let tail: Vec<u32> = (0..10).map(|_| restored.next_u32()).collect();
        assert_eq!(tail, expected[10..]);
    }

    #[test]
    fn derive_sub_seed_advances_main_stream() {
        let mut main = Pcg64Rng::from_seed(1);
        let first = main.next_u32();
        let battle_seed = main.derive_sub_seed(0);
        assert_ne!(battle_seed, 0);

        let mut replay = Pcg64Rng::from_seed(1);
        assert_eq!(replay.next_u32(), first);
        let expected_battle = replay.next_u64();
        assert_eq!(battle_seed, expected_battle);
    }

    #[test]
    fn advance_matches_replayed_draws() {
        for steps in [0_u64, 1, 10, 1_000, 5_000] {
            let mut replay = Pcg64Rng::from_seed(7);
            for _ in 0..steps {
                let _ = replay.next_u32();
            }
            let expected = replay.next_u32();

            let mut restored = Pcg64Rng::from_seed(7);
            restored.restore_state(RngState { seed: 7, steps });
            assert_eq!(restored.next_u32(), expected, "steps={steps}");
        }
    }
}
