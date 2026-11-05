//! Bundled battle content (types, moves, species) for tests and loaders.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::error::{DataError, Result};
use crate::move_def::MoveDef;
use crate::species::SpeciesDef;
use crate::type_chart::TypeChart;

/// In-memory battle content bundle.
#[derive(Clone, Debug, PartialEq)]
pub struct BattlePack {
    /// Type effectiveness chart.
    pub type_chart: TypeChart,
    /// Moves keyed by id.
    pub moves: BTreeMap<String, MoveDef>,
    /// Species keyed by id.
    pub species: BTreeMap<String, SpeciesDef>,
}

impl BattlePack {
    /// Creates an empty pack (tests build programmatically).
    pub fn new(type_chart: TypeChart) -> Self {
        Self {
            type_chart,
            moves: BTreeMap::new(),
            species: BTreeMap::new(),
        }
    }

    /// Inserts a move (errors on duplicate id).
    pub fn insert_move(&mut self, mv: MoveDef) -> Result<()> {
        match self.moves.entry(mv.id.clone()) {
            Entry::Occupied(_) => Err(DataError::Duplicate {
                kind: "move".into(),
                id: mv.id,
            }),
            Entry::Vacant(v) => {
                v.insert(mv);
                Ok(())
            }
        }
    }

    /// Inserts a species.
    pub fn insert_species(&mut self, sp: SpeciesDef) -> Result<()> {
        match self.species.entry(sp.id.clone()) {
            Entry::Occupied(_) => Err(DataError::Duplicate {
                kind: "species".into(),
                id: sp.id,
            }),
            Entry::Vacant(v) => {
                v.insert(sp);
                Ok(())
            }
        }
    }

    /// Looks up a move by id.
    pub fn move_by_id(&self, id: &str) -> Option<&MoveDef> {
        self.moves.get(id)
    }
}
