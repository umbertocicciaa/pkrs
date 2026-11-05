//! Entity occupancy checks during movement (§6.3).

use pkrs_core::GridPos;

/// Tiles blocked by other entities (NPCs, players).
pub trait Occupancy {
    /// `true` when another entity occupies `pos`.
    fn blocks(&self, pos: GridPos) -> bool;
}

impl Occupancy for () {
    fn blocks(&self, _pos: GridPos) -> bool {
        false
    }
}

impl<F> Occupancy for F
where
    F: Fn(GridPos) -> bool,
{
    fn blocks(&self, pos: GridPos) -> bool {
        self(pos)
    }
}
