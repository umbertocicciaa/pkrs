//! Battle RNG policy (§7.2).

use pkrs_core::{GameRng, Pcg64Rng};

/// How randomness is sourced for a battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RngPolicy {
    /// Derived from the save game RNG at battle start (`derive_sub_seed` on the parent stream).
    /// The stored seed is the battle stream seed returned by [`Self::local_from_game_rng`].
    Local {
        /// Battle stream seed (not the save root seed).
        parent_seed: u64,
    },
    /// Fully pinned seed (tests, Battle Factory).
    Fixed {
        /// Battle seed.
        seed: u64,
    },
    /// Server-owned stream placeholder for future networked battles (§7.2).
    Authoritative {
        /// Authoritative battle seed from the server.
        seed: u64,
        /// Identifies which sub-stream to open on the server.
        stream_id: u32,
    },
}

impl RngPolicy {
    /// Derives a single-player policy from the save's main RNG (advances the save stream).
    #[must_use]
    pub fn local_from_game_rng(game: &mut impl GameRng) -> Self {
        Self::Local {
            parent_seed: game.derive_sub_seed(0x00BA_771E),
        }
    }

    /// Seed of the AI decision stream for `side`, independent of the battle stream so AI
    /// rolls never shift damage rolls and recorded inputs replay without the AI (§7.5).
    #[must_use]
    pub fn ai_seed(&self, side: crate::model::SideId) -> u64 {
        let base = match self {
            Self::Local { parent_seed } | Self::Fixed { seed: parent_seed } => *parent_seed,
            Self::Authoritative { seed, stream_id } => *seed ^ u64::from(*stream_id).rotate_left(9),
        };
        let label: u64 = match side {
            crate::model::SideId::Player => 0x00A1_0001,
            crate::model::SideId::Opponent => 0x00A1_0002,
        };
        Pcg64Rng::from_seed(base ^ label.rotate_left(40)).derive_sub_seed(label)
    }

    /// Opens a battle-local RNG stream.
    pub fn open(&self) -> Pcg64Rng {
        match self {
            Self::Local { parent_seed } | Self::Fixed { seed: parent_seed } => {
                Pcg64Rng::from_seed(*parent_seed)
            }
            Self::Authoritative { seed, stream_id } => {
                Pcg64Rng::from_seed(*seed ^ u64::from(*stream_id).rotate_left(17))
            }
        }
    }
}

/// Battle-owned RNG wrapper.
pub struct BattleRng {
    inner: Pcg64Rng,
}

impl BattleRng {
    /// Creates an RNG from `policy`.
    pub fn new(policy: RngPolicy) -> Self {
        Self {
            inner: policy.open(),
        }
    }

    /// Mutable access to the port (crit rolls, accuracy, …).
    pub fn as_game_rng(&mut self) -> &mut dyn GameRng {
        &mut self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::Pcg64Rng;

    #[test]
    fn local_from_game_advances_main_stream() {
        let mut game = Pcg64Rng::from_seed(42);
        let before = game.state();
        let policy = RngPolicy::local_from_game_rng(&mut game);
        let after = game.state();
        assert_eq!(after.steps, before.steps + 2, "derive uses next_u64");
        match policy {
            RngPolicy::Local { parent_seed } => assert_ne!(parent_seed, 0),
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn fixed_and_local_open_are_stable() {
        let policy = RngPolicy::Fixed { seed: 99 };
        let a = policy.open().next_u32();
        let b = policy.open().next_u32();
        assert_eq!(a, b);
    }

    #[test]
    fn authoritative_streams_differ_by_stream_id() {
        let a = RngPolicy::Authoritative {
            seed: 1,
            stream_id: 0,
        };
        let b = RngPolicy::Authoritative {
            seed: 1,
            stream_id: 1,
        };
        assert_ne!(a.open().next_u32(), b.open().next_u32());
    }

    #[test]
    fn ai_seed_is_independent_per_side_and_policy() {
        let fixed = RngPolicy::Fixed { seed: 7 };
        assert_ne!(
            fixed.ai_seed(crate::model::SideId::Player),
            fixed.ai_seed(crate::model::SideId::Opponent)
        );
        let other = RngPolicy::Fixed { seed: 8 };
        assert_ne!(
            fixed.ai_seed(crate::model::SideId::Player),
            other.ai_seed(crate::model::SideId::Player)
        );
    }
}
