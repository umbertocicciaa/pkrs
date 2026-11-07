//! `maps/world.ron`: seamless map connections (§6.4).

use std::collections::BTreeMap;

use pkrs_assets::{Vfs, VfsPath};
use pkrs_core::{Direction, GridPos};
use serde::Deserialize;

use crate::{Result, WorldError};

/// Default world layout path inside the project VFS.
pub const WORLD_RON: &str = "maps/world.ron";

/// A neighbour map reachable across one edge of the source map.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeighborLink {
    /// Destination map id (`.tmx` stem).
    pub map: String,
    /// Tile offset applied when translating coordinates onto the neighbour (§6.4).
    #[serde(default)]
    pub offset: (i32, i32),
}

/// Per-map links along the four cardinal edges.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MapLinks {
    /// Neighbour across the north edge (`y = 0` when walking up).
    #[serde(default)]
    pub north: Option<NeighborLink>,
    /// Neighbour across the south edge.
    #[serde(default)]
    pub south: Option<NeighborLink>,
    /// Neighbour across the east edge.
    #[serde(default)]
    pub east: Option<NeighborLink>,
    /// Neighbour across the west edge.
    #[serde(default)]
    pub west: Option<NeighborLink>,
}

impl MapLinks {
    /// Link across `dir` from this map, if declared.
    #[must_use]
    pub fn link(&self, dir: Direction) -> Option<&NeighborLink> {
        match dir {
            Direction::Up => self.north.as_ref(),
            Direction::Down => self.south.as_ref(),
            Direction::Left => self.west.as_ref(),
            Direction::Right => self.east.as_ref(),
        }
    }
}

/// Parsed `maps/world.ron` (map id → edge links).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct WorldLayout {
    #[serde(flatten)]
    maps: BTreeMap<String, MapLinks>,
}

impl WorldLayout {
    /// Loads `maps/world.ron` when present; `Ok(None)` if the file is missing.
    pub fn load_optional(vfs: &dyn Vfs) -> Result<Option<Self>> {
        let path = VfsPath::new(WORLD_RON)?;
        if !vfs.exists(&path) {
            return Ok(None);
        }
        let bytes = vfs.read(&path)?;
        let text = std::str::from_utf8(&bytes).map_err(|e| WorldError::LayoutParse {
            file: WORLD_RON.into(),
            message: format!("not UTF-8: {e}"),
        })?;
        Self::parse(text, WORLD_RON).map(Some)
    }

    /// Parses RON source (implicit Some for optional neighbour fields).
    pub fn parse(source: &str, file: &str) -> Result<Self> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(source)
            .map_err(|e| WorldError::LayoutParse {
                file: file.into(),
                message: e.to_string(),
            })
    }

    /// Links declared for `map_id`, if any.
    #[must_use]
    pub fn links_for(&self, map_id: &str) -> Option<&MapLinks> {
        self.maps.get(map_id)
    }

    /// Neighbour ids referenced from `map_id` (deduplicated, stable order).
    pub fn neighbor_ids<'a>(&'a self, map_id: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        let links = self.maps.get(map_id);
        [
            links.and_then(|l| l.north.as_ref()),
            links.and_then(|l| l.south.as_ref()),
            links.and_then(|l| l.east.as_ref()),
            links.and_then(|l| l.west.as_ref()),
        ]
        .into_iter()
        .flatten()
        .map(|n| n.map.as_str())
    }

    /// When stepping from `pos` on `from_map` towards `dir` leaves the map, returns the
    /// neighbour map id and local entry tile (after `offset`).
    #[must_use]
    pub fn crossing(
        &self,
        from_map: &str,
        pos: GridPos,
        dir: Direction,
        from_size: (u32, u32),
        to_size: (u32, u32),
    ) -> Option<(&str, GridPos)> {
        if !on_departure_edge(pos, dir, from_size) {
            return None;
        }
        let links = self.maps.get(from_map)?;
        let link = links.link(dir)?;
        if link.map.is_empty() {
            return None;
        }
        let entry = translate_entry(pos, dir, link.offset, to_size)?;
        Some((link.map.as_str(), entry))
    }
}

/// `true` when `pos` lies on the map edge crossed by a step in `dir`.
#[must_use]
pub fn on_departure_edge(pos: GridPos, dir: Direction, size: (u32, u32)) -> bool {
    let (w, h) = size;
    if w == 0 || h == 0 {
        return false;
    }
    let max_x = i32::try_from(w.saturating_sub(1)).unwrap_or(i32::MAX);
    let max_y = i32::try_from(h.saturating_sub(1)).unwrap_or(i32::MAX);
    match dir {
        Direction::Up => pos.y == 0,
        Direction::Down => pos.y == max_y,
        Direction::Left => pos.x == 0,
        Direction::Right => pos.x == max_x,
    }
}

/// Entry tile on the neighbour map for an edge crossing.
#[must_use]
pub fn translate_entry(
    exit: GridPos,
    dir: Direction,
    offset: (i32, i32),
    to_size: (u32, u32),
) -> Option<GridPos> {
    let (w, h) = to_size;
    let max_x = i32::try_from(w.saturating_sub(1)).ok()?;
    let max_y = i32::try_from(h.saturating_sub(1)).ok()?;
    let (ox, oy) = offset;
    let entry = match dir {
        Direction::Up => GridPos::new(exit.x + ox, max_y + oy),
        Direction::Down => GridPos::new(exit.x + ox, oy),
        Direction::Left => GridPos::new(max_x + ox, exit.y + oy),
        Direction::Right => GridPos::new(ox, exit.y + oy),
    };
    in_map(entry, to_size).then_some(entry)
}

fn in_map(pos: GridPos, size: (u32, u32)) -> bool {
    let (w, h) = size;
    pos.x >= 0
        && pos.y >= 0
        && u32::try_from(pos.x).ok().is_some_and(|x| x < w)
        && u32::try_from(pos.y).ok().is_some_and(|y| y < h)
}

/// Pixel offset for drawing a neighbour map around the active map at the origin.
#[must_use]
pub fn neighbor_pixel_offset(
    dir: Direction,
    main_size: (u32, u32),
    neighbor_size: (u32, u32),
    tile: (u32, u32),
    offset: (i32, i32),
) -> (f32, f32) {
    let (tw, th) = tile;
    let (mw, mh) = main_size;
    let (nw, nh) = neighbor_size;
    let (ox, oy) = offset;
    let ox_px = ox as f32 * tw as f32;
    let oy_px = oy as f32 * th as f32;
    match dir {
        Direction::Up => (ox_px, -(nh as f32 * th as f32) + oy_px),
        Direction::Down => (ox_px, mh as f32 * th as f32 + oy_px),
        Direction::Left => (-(nw as f32 * tw as f32) + ox_px, oy_px),
        Direction::Right => (mw as f32 * tw as f32 + ox_px, oy_px),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: &str = r#"{
        route_a: (
            south: (map: "route_b", offset: (0, 0)),
        ),
        route_b: (
            north: (map: "route_a", offset: (0, 0)),
        ),
    }"#;

    #[test]
    fn parse_world_ron() {
        let w = WorldLayout::parse(LAYOUT, "world.ron").unwrap();
        assert_eq!(
            w.links_for("route_a").unwrap().south.as_ref().unwrap().map,
            "route_b"
        );
    }

    #[test]
    fn south_link_lands_on_neighbour_north_edge() {
        let w = WorldLayout::parse(LAYOUT, "world.ron").unwrap();
        let exit = GridPos::new(5, 13);
        let (to_map, entry) = w
            .crossing("route_a", exit, Direction::Down, (20, 14), (20, 10))
            .unwrap();
        assert_eq!(to_map, "route_b");
        assert_eq!(entry, GridPos::new(5, 0));
    }

    #[test]
    fn offset_shifts_entry_coordinate() {
        let pos = GridPos::new(3, 0);
        assert_eq!(
            translate_entry(pos, Direction::Up, (2, 0), (10, 8)).unwrap(),
            GridPos::new(5, 7)
        );
    }

    #[test]
    fn crossing_rejects_non_edge_positions() {
        let w = WorldLayout::parse(LAYOUT, "world.ron").unwrap();
        assert!(
            w.crossing(
                "route_a",
                GridPos::new(5, 3),
                Direction::Up,
                (20, 14),
                (20, 10)
            )
            .is_none()
        );
    }
}
