//! Plugin terrain rules (§6.3).

use std::collections::HashMap;

use pkrs_core::{Direction, GridPos};

use super::field::FieldState;

/// Result of a custom terrain rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainStepVerdict {
    /// Allow the step (after collision checks).
    Allow,
    /// Block movement into the tile (turn-only still allowed).
    Block,
}

/// Custom terrain handler registered by id.
pub type TerrainRuleFn =
    fn(field: &FieldState, from: GridPos, dir: Direction) -> TerrainStepVerdict;

/// Registry of plugin terrain rules keyed by custom terrain name.
#[derive(Clone, Debug, Default)]
pub struct TerrainRuleRegistry {
    rules: HashMap<String, TerrainRuleFn>,
}

impl TerrainRuleRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `rule` for terrain id `name` (must match [`crate::Terrain::Custom`]).
    pub fn register(&mut self, name: impl Into<String>, rule: TerrainRuleFn) {
        self.rules.insert(name.into(), rule);
    }

    /// Looks up a custom rule.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<TerrainRuleFn> {
        self.rules.get(name).copied()
    }
}
