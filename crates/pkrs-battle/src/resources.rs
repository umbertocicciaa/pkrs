//! Content loaded from `pkrs-data` for one battle instance.

use pkrs_data::BattlePack;

/// Type chart, moves and species used to resolve actions (§7.3).
#[derive(Clone, Debug, PartialEq)]
pub struct BattleResources {
    /// Parsed content bundle.
    pub pack: BattlePack,
}

impl BattleResources {
    /// Wraps a [`BattlePack`].
    pub fn new(pack: BattlePack) -> Self {
        Self { pack }
    }
}
