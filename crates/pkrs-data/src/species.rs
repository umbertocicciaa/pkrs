//! Minimal species schema for battles (full data in M6).

use serde::Deserialize;

use crate::error::{DataError, Result};

/// Species record used when building battle parties from content data.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SpeciesDef {
    /// Content id.
    pub id: String,
    /// One or two type ids.
    pub types: Vec<String>,
    /// Base HP.
    pub hp: u32,
    /// Base attack.
    pub attack: u32,
    /// Base defense.
    pub defense: u32,
    /// Base speed.
    pub speed: u32,
}

impl SpeciesDef {
    /// Parses a species RON document.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        ron::from_str(source).map_err(|e| DataError::Parse {
            file: file.into(),
            message: e.to_string(),
        })
    }
}
