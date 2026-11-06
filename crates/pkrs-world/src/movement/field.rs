//! Field abilities that gate terrain (§6.3).

/// Overworld field state consulted by terrain rules (e.g. Surf for water).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldState {
    /// Player (or leader) may enter water tiles.
    pub surf: bool,
}
