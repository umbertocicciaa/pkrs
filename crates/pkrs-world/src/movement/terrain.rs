//! Custom terrain rules (§6.3).
//!
//! The stepper asks a [`TerrainRules`] port whenever a step would enter a
//! [`Terrain::Custom`](crate::Terrain::Custom) tile. [`TerrainRuleRegistry`] is the in-engine
//! implementation (plain Rust functions); the runtime layers plugin rules (Lua
//! `world.register_terrain` and data-defined terrains) on top of it.

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

/// A step onto a custom terrain tile, as a rule sees it.
#[derive(Clone, Copy, Debug)]
pub struct TerrainQuery<'a> {
    /// Terrain name of the destination tile.
    pub terrain: &'a str,
    /// Field state of the walker.
    pub field: &'a FieldState,
    /// Tile the step starts on.
    pub from: GridPos,
    /// Destination tile.
    pub to: GridPos,
    /// Step direction.
    pub dir: Direction,
}

/// Port: decides whether a step onto a custom terrain is allowed.
pub trait TerrainRules {
    /// Verdict for `query`; terrains without a rule are [`TerrainStepVerdict::Allow`].
    fn verdict(&self, query: &TerrainQuery<'_>) -> TerrainStepVerdict;
}

/// Custom terrain handler registered by id.
pub type TerrainRuleFn =
    fn(field: &FieldState, from: GridPos, dir: Direction) -> TerrainStepVerdict;

/// Registry of Rust terrain rules keyed by custom terrain name.
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

impl TerrainRules for TerrainRuleRegistry {
    fn verdict(&self, query: &TerrainQuery<'_>) -> TerrainStepVerdict {
        self.get(query.terrain)
            .map_or(TerrainStepVerdict::Allow, |rule| {
                rule(query.field, query.from, query.dir)
            })
    }
}
