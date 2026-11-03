//! Integer tile coordinates on the overworld grid.

use core::fmt;

use crate::direction::Direction;

/// A position on the tile grid (`x` right, `y` down).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct GridPos {
    /// Column index.
    pub x: i32,
    /// Row index.
    pub y: i32,
}

impl GridPos {
    /// Creates a position from tile coordinates.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Returns the neighbouring tile in `dir`.
    pub fn neighbour(self, dir: Direction) -> Self {
        let (dx, dy) = dir.delta();
        self.offset(dx, dy)
    }

    /// Translates by `(dx, dy)` tiles.
    pub fn offset(self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
        }
    }

    /// Manhattan distance to `other`.
    pub fn manhattan(self, other: Self) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }
}

impl fmt::Display for GridPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbour_matches_offset() {
        let start = GridPos::new(3, 4);
        assert_eq!(start.neighbour(Direction::Up), GridPos::new(3, 3));
        assert_eq!(start.offset(1, -2), GridPos::new(4, 2));
    }
}
