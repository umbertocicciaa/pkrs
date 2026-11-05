//! The logical grid: collision, directional blocking and terrain (§1.4, §6.2).
//!
//! The logical grid is independent of the visual tile layers so a future renderer (or a 3D map
//! mode) can draw the same map differently without touching movement rules.

use pkrs_core::{Direction, GridPos};

/// Edges of a tile that cannot be crossed (`block_n/s/e/w`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EdgeMask(u8);

impl EdgeMask {
    /// No blocked edges.
    pub const NONE: Self = Self(0);
    /// All four edges blocked.
    pub const ALL: Self = Self(0b1111);

    const fn bit(dir: Direction) -> u8 {
        match dir {
            Direction::Up => 0b0001,
            Direction::Down => 0b0010,
            Direction::Left => 0b0100,
            Direction::Right => 0b1000,
        }
    }

    /// Mask with only `dir` blocked.
    #[must_use]
    pub const fn only(dir: Direction) -> Self {
        Self(Self::bit(dir))
    }

    /// This mask with `dir` blocked as well.
    #[must_use]
    pub const fn with(self, dir: Direction) -> Self {
        Self(self.0 | Self::bit(dir))
    }

    /// Union of two masks.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `true` when the edge on the `dir` side is blocked.
    #[must_use]
    pub const fn blocks(self, dir: Direction) -> bool {
        self.0 & Self::bit(dir) != 0
    }

    /// `true` when no edge is blocked.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Terrain kinds from the tile property `terrain=` (§6.2, §6.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Terrain {
    /// Short grass (decorative, no encounters unless a rect says so).
    Grass,
    /// Tall grass: encounter terrain.
    TallGrass,
    /// Water: requires the `surf` field state.
    Water,
    /// Ice: forced slide until blocked.
    Ice,
    /// One-way ledge, crossable only towards the given direction.
    Ledge(Direction),
    /// A terrain registered by a plugin (`world.register_terrain`).
    Custom(String),
}

impl Terrain {
    /// Parses a `terrain=` value. Unknown names become [`Terrain::Custom`] so plugins can
    /// register them; only malformed built-ins (e.g. `ledge_x`) are rejected.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let terrain = match raw {
            "grass" => Self::Grass,
            "tall_grass" => Self::TallGrass,
            "water" => Self::Water,
            "ice" => Self::Ice,
            "ledge_n" => Self::Ledge(Direction::Up),
            "ledge_s" => Self::Ledge(Direction::Down),
            "ledge_w" => Self::Ledge(Direction::Left),
            "ledge_e" => Self::Ledge(Direction::Right),
            other if other.starts_with("ledge") => {
                return Err(format!(
                    "unknown ledge direction in `{other}` (use ledge_n/s/e/w)"
                ));
            }
            "" => return Err("terrain name is empty".into()),
            other => Self::Custom(other.to_owned()),
        };
        Ok(terrain)
    }

    /// The `terrain=` spelling of this terrain.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Grass => "grass",
            Self::TallGrass => "tall_grass",
            Self::Water => "water",
            Self::Ice => "ice",
            Self::Ledge(Direction::Up) => "ledge_n",
            Self::Ledge(Direction::Down) => "ledge_s",
            Self::Ledge(Direction::Left) => "ledge_w",
            Self::Ledge(Direction::Right) => "ledge_e",
            Self::Custom(name) => name,
        }
    }
}

/// Logical data for one tile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    /// Fully blocked (`collision` layer tile or `solid=true`).
    pub solid: bool,
    /// Edges that cannot be crossed in or out.
    pub blocked_edges: EdgeMask,
    /// Terrain rule for the tile, if any.
    pub terrain: Option<Terrain>,
}

/// Width × height grid of [`Cell`]s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalGrid {
    width: u32,
    height: u32,
    cells: Vec<Cell>,
}

impl LogicalGrid {
    /// An all-walkable grid.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            cells: vec![Cell::default(); len],
        }
    }

    /// Width in tiles.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in tiles.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// `true` when `pos` lies inside the grid.
    #[must_use]
    pub fn in_bounds(&self, pos: GridPos) -> bool {
        self.index(pos).is_some()
    }

    fn index(&self, pos: GridPos) -> Option<usize> {
        let x = u32::try_from(pos.x).ok()?;
        let y = u32::try_from(pos.y).ok()?;
        (x < self.width && y < self.height)
            .then(|| (y as usize) * (self.width as usize) + x as usize)
    }

    /// Cell at `pos` (`None` out of bounds).
    #[must_use]
    pub fn cell(&self, pos: GridPos) -> Option<&Cell> {
        self.index(pos).and_then(|i| self.cells.get(i))
    }

    /// Mutable cell at `pos` (`None` out of bounds).
    pub fn cell_mut(&mut self, pos: GridPos) -> Option<&mut Cell> {
        let i = self.index(pos)?;
        self.cells.get_mut(i)
    }

    /// `true` when the tile at `pos` is out of bounds or solid.
    #[must_use]
    pub fn is_solid(&self, pos: GridPos) -> bool {
        self.cell(pos).is_none_or(|c| c.solid)
    }

    /// Terrain at `pos`.
    #[must_use]
    pub fn terrain(&self, pos: GridPos) -> Option<&Terrain> {
        self.cell(pos).and_then(|c| c.terrain.as_ref())
    }

    /// Collision-only check for a one-tile step from `from` towards `dir`: bounds, solid
    /// target, and directional edges on both sides of the shared edge.
    ///
    /// Terrain rules (ledges, water, ice) and entity occupancy are applied on top by grid
    /// movement (§6.3).
    #[must_use]
    pub fn can_cross(&self, from: GridPos, dir: Direction) -> bool {
        let to = from.neighbour(dir);
        let (Some(src), Some(dst)) = (self.cell(from), self.cell(to)) else {
            return false;
        };
        !dst.solid && !src.blocked_edges.blocks(dir) && !dst.blocked_edges.blocks(dir.opposite())
    }

    /// Iterates `(pos, cell)` in row-major order.
    pub fn iter(&self) -> impl Iterator<Item = (GridPos, &Cell)> {
        let width = self.width.max(1) as usize;
        self.cells.iter().enumerate().map(move |(i, cell)| {
            let x = i32::try_from(i % width).unwrap_or(i32::MAX);
            let y = i32::try_from(i / width).unwrap_or(i32::MAX);
            (GridPos::new(x, y), cell)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_mask_blocks_only_its_edges() {
        let mask = EdgeMask::only(Direction::Up).with(Direction::Left);
        assert!(mask.blocks(Direction::Up));
        assert!(mask.blocks(Direction::Left));
        assert!(!mask.blocks(Direction::Down));
        assert!(!mask.blocks(Direction::Right));
        assert!(EdgeMask::NONE.is_empty());
        assert!(EdgeMask::ALL.blocks(Direction::Right));
    }

    #[test]
    fn terrain_parses_builtins_and_custom() {
        assert_eq!(Terrain::parse("tall_grass"), Ok(Terrain::TallGrass));
        assert_eq!(
            Terrain::parse("ledge_s"),
            Ok(Terrain::Ledge(Direction::Down))
        );
        assert_eq!(Terrain::parse("mud"), Ok(Terrain::Custom("mud".into())));
        assert!(Terrain::parse("ledge_x").is_err());
        assert!(Terrain::parse("").is_err());
        for name in [
            "grass",
            "tall_grass",
            "water",
            "ice",
            "ledge_n",
            "ledge_s",
            "ledge_e",
            "ledge_w",
            "mud",
        ] {
            assert_eq!(
                Terrain::parse(name).map(|t| t.name().to_owned()),
                Ok(name.into())
            );
        }
    }

    #[test]
    fn solid_and_bounds_block_steps() {
        let mut grid = LogicalGrid::new(3, 3);
        grid.cell_mut(GridPos::new(1, 0)).unwrap().solid = true;
        let origin = GridPos::new(0, 0);
        assert!(!grid.can_cross(origin, Direction::Right), "solid target");
        assert!(grid.can_cross(origin, Direction::Down));
        assert!(!grid.can_cross(origin, Direction::Up), "out of bounds");
        assert!(!grid.can_cross(origin, Direction::Left), "out of bounds");
        assert!(grid.is_solid(GridPos::new(-1, 0)));
    }

    #[test]
    fn directional_blocking_checks_both_sides_of_the_edge() {
        let mut grid = LogicalGrid::new(3, 3);
        // A fence on the south edge of (1, 1).
        grid.cell_mut(GridPos::new(1, 1)).unwrap().blocked_edges = EdgeMask::only(Direction::Down);
        // Leaving (1, 1) southwards is blocked...
        assert!(!grid.can_cross(GridPos::new(1, 1), Direction::Down));
        // ...and so is entering (1, 1) from the south.
        assert!(!grid.can_cross(GridPos::new(1, 2), Direction::Up));
        // Other edges are open.
        assert!(grid.can_cross(GridPos::new(1, 1), Direction::Up));
        assert!(grid.can_cross(GridPos::new(0, 1), Direction::Right));
    }

    #[test]
    fn iter_is_row_major() {
        let grid = LogicalGrid::new(2, 2);
        let positions: Vec<_> = grid.iter().map(|(p, _)| p).collect();
        assert_eq!(
            positions,
            [
                GridPos::new(0, 0),
                GridPos::new(1, 0),
                GridPos::new(0, 1),
                GridPos::new(1, 1)
            ]
        );
    }
}
