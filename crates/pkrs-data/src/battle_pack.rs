//! Bundled battle content (types, moves, species) for tests and loaders.

use std::collections::BTreeMap;

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
        if self.moves.insert(mv.id.clone(), mv).is_some() {
            return Err(DataError::Parse {
                file: "battle_pack".into(),
                message: "duplicate move id".into(),
            });
        }
        Ok(())
    }

    /// Inserts a species.
    pub fn insert_species(&mut self, sp: SpeciesDef) -> Result<()> {
        if self.species.insert(sp.id.clone(), sp).is_some() {
            return Err(DataError::Parse {
                file: "battle_pack".into(),
                message: "duplicate species id".into(),
            });
        }
        Ok(())
    }

    /// Looks up a move by id.
    pub fn move_by_id(&self, id: &str) -> Option<&MoveDef> {
        self.moves.get(id)
    }
}
