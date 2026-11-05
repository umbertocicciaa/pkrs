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
    /// Seed of the AI decision stream for `side`, independent of the battle stream so AI
    /// rolls never shift damage rolls and recorded inputs replay without the AI (§7.5).
    #[must_use]
    pub fn ai_seed(&self, side: crate::model::SideId) -> u64 {
        let base = match self {
            Self::Local { parent_seed } => *parent_seed,
            Self::Fixed { seed } => *seed,
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
