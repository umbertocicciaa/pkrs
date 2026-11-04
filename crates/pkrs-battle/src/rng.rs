//! Battle RNG policy (§7.2).

use pkrs_core::{GameRng, Pcg64Rng};

/// How randomness is sourced for a battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RngPolicy {
    /// Derived from the save game RNG (`derive_sub_seed` on the parent stream).
    Local {
        /// Parent seed snapshot.
        parent_seed: u64,
    },
    /// Fully pinned seed (tests, Battle Factory).
    Fixed {
        /// Battle seed.
        seed: u64,
    },
}

impl RngPolicy {
    /// Opens a battle-local RNG stream.
    pub fn open(&self) -> Pcg64Rng {
        match self {
            Self::Local { parent_seed } => {
                let mut parent = Pcg64Rng::from_seed(*parent_seed);
                let sub = parent.derive_sub_seed(0x00BA_771E);
                Pcg64Rng::from_seed(sub)
            }
            Self::Fixed { seed } => Pcg64Rng::from_seed(*seed),
        }
    }
}

/// Battle-owned RNG wrapper (reserved for damage rolls in later issues).
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
