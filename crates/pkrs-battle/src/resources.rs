//! Content loaded from `pkrs-data` for one battle instance.

use pkrs_data::{BattlePack, BattleRules};

/// Type chart, moves, species, and field rules used to resolve actions (§7.3).
#[derive(Clone, Debug, PartialEq)]
pub struct BattleResources {
    /// Parsed content bundle.
    pub pack: BattlePack,
    /// Status / weather behaviour catalog.
    pub rules: BattleRules,
}

impl BattleResources {
    /// Wraps a [`BattlePack`] with default Gen-3 rules.
    pub fn new(pack: BattlePack) -> Self {
        Self {
            pack,
            rules: BattleRules::gen3_defaults(),
        }
    }

    /// Wraps pack and explicit rules (tests / custom content).
    pub fn with_rules(pack: BattlePack, rules: BattleRules) -> Self {
        Self { pack, rules }
    }
}
