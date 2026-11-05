//! Type list and effectiveness matrix (`data/types.ron`, §7.3).

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::error::{DataError, Result};

/// Parsed type chart.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeChart {
    types: Vec<String>,
    effectiveness: HashMap<(String, String), f64>,
}

impl TypeChart {
    /// Builds a chart from parts (tests and programmatic loaders).
    pub fn from_parts(types: Vec<String>, effectiveness: HashMap<(String, String), f64>) -> Self {
        Self {
            types,
            effectiveness,
        }
    }

    /// All type ids in declaration order.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// Multiplier when `move_type` hits a defender with `defender_type`.
    pub fn multiplier(&self, move_type: &str, defender_type: &str) -> f64 {
        self.effectiveness
            .get(&(move_type.to_string(), defender_type.to_string()))
            .copied()
            .unwrap_or(1.0)
    }

    /// Combined multiplier against one or two defender types (product).
    pub fn combined_multiplier(&self, move_type: &str, defender_types: &[String]) -> f64 {
        if defender_types.is_empty() {
            return 1.0;
        }
        defender_types
            .iter()
            .map(|t| self.multiplier(move_type, t))
            .product()
    }

    /// Parses the `types.ron` document shape used in project fixtures.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        let raw: RawTypeChart = ron::from_str(source).map_err(|e| DataError::Parse {
            file: file.into(),
            message: e.to_string(),
        })?;
        raw.into_chart(file)
    }
}

#[derive(Debug, Deserialize)]
struct RawTypeChart {
    types: Vec<String>,
    chart: HashMap<(String, String), f64>,
}

impl RawTypeChart {
    fn into_chart(self, file: &str) -> Result<TypeChart> {
        let mut seen = HashSet::new();
        for t in &self.types {
            if !seen.insert(t.clone()) {
                return Err(DataError::Parse {
                    file: file.into(),
                    message: format!("duplicate type `{t}`"),
                });
            }
        }
        for (atk, def) in self.chart.keys() {
            if !self.types.contains(atk) {
                return Err(DataError::Parse {
                    file: file.into(),
                    message: format!("chart references unknown attacking type `{atk}`"),
                });
            }
            if !self.types.contains(def) {
                return Err(DataError::Parse {
                    file: file.into(),
                    message: format!("chart references unknown defending type `{def}`"),
                });
            }
        }
        for ((atk, def), &mult) in &self.chart {
            if !mult.is_finite() || mult < 0.0 {
                return Err(DataError::Parse {
                    file: file.into(),
                    message: format!(
                        "chart entry ({atk}, {def}) must be a finite value >= 0 (got {mult})"
                    ),
                });
            }
        }
        // TODO(M6): cross-reference move/species type ids against `types`.
        Ok(TypeChart {
            types: self.types,
            effectiveness: self.chart,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_resolves_immunity_and_weakness() {
        let chart = TypeChart::parse_ron(
            r#"(
                types: ["normal", "fire", "water", "ghost"],
                chart: {
                    ("fire", "water"): 0.5,
                    ("water", "fire"): 2.0,
                    ("normal", "ghost"): 0.0,
                    ("fire", "fire"): 0.5,
                },
            )"#,
            "types.ron",
        )
        .expect("parse");
        assert!((chart.multiplier("fire", "water") - 0.5).abs() < f64::EPSILON);
        assert!((chart.multiplier("water", "fire") - 2.0).abs() < f64::EPSILON);
        assert!((chart.multiplier("normal", "ghost") - 0.0).abs() < f64::EPSILON);
        assert!((chart.multiplier("fire", "fire") - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_invalid_multiplier() {
        let err = TypeChart::parse_ron(
            r#"(
                types: ["a", "b"],
                chart: { ("a", "b"): -1.0 },
            )"#,
            "bad.ron",
        )
        .expect_err("parse");
        assert!(err.to_string().contains("bad.ron"));
    }
}
