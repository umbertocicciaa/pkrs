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
        let layout: Self = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(source)
            .map_err(|e| WorldError::LayoutParse {
                file: file.into(),
                message: e.to_string(),
            })?;
        layout.validate_layout(file, &BTreeMap::new())?;
        Ok(layout)
    }

    /// Validates reciprocity, map ids, offsets, and entry tiles (§6.4).
    pub fn validate_layout(
        &self,
        file: &str,
        map_sizes: &BTreeMap<String, (u32, u32)>,
    ) -> Result<()> {
        for (map_id, links) in &self.maps {
            if map_id.is_empty() {
                return Err(WorldError::LayoutInvalid {
                    file: file.into(),
                    message: "map id must not be empty".into(),
                });
            }
            if map_sizes.contains_key(map_id) {
                validate_links_for_map(map_id, links, file, map_sizes)?;
            }
            for (dir, link) in cardinals(links) {
                let Some(link) = link else { continue };
                if link.map.is_empty() {
                    return Err(WorldError::LayoutInvalid {
                        file: file.into(),
                        message: format!("map `{map_id}` {:?}: destination id is empty", dir),
                    });
                }
                if link.map == *map_id {
                    return Err(WorldError::LayoutInvalid {
                        file: file.into(),
                        message: format!("map `{map_id}` {:?}: self-link", dir),
                    });
                }
                if !offset_along_edge_only(dir, link.offset) {
                    return Err(WorldError::LayoutInvalid {
                        file: file.into(),
                        message: format!(
                            "map `{map_id}` {:?}: offset {:?} has a non-zero perpendicular component",
                            dir, link.offset
                        ),
                    });
                }
                if let Some(to_size) = map_sizes.get(&link.map) {
                    if !sample_entry_in_map(dir, link.offset, *to_size) {
                        return Err(WorldError::LayoutInvalid {
                            file: file.into(),
                            message: format!(
                                "map `{map_id}` {:?} → `{}`: offset {:?} puts no seam tile in bounds",
                                dir, link.map, link.offset
                            ),
                        });
                    }
                }
                if let (Some(from_size), Some(to_size)) =
                    (map_sizes.get(map_id), map_sizes.get(&link.map))
                {
                    let reverse = self.maps.get(&link.map);
                    let reciprocal = reverse.and_then(|l| l.link(dir.opposite()));
                    match reciprocal {
                        None => {
                            return Err(WorldError::LayoutInvalid {
                                file: file.into(),
                                message: format!(
                                    "map `{map_id}` {:?} → `{}` but `{}` has no {:?} link back",
                                    dir,
                                    link.map,
                                    link.map,
                                    dir.opposite()
                                ),
                            });
                        }
                        Some(back) if back.map != *map_id => {
                            return Err(WorldError::LayoutInvalid {
                                file: file.into(),
                                message: format!(
                                    "map `{map_id}` {:?} → `{}` but `{}` {:?} points to `{}`",
                                    dir,
                                    link.map,
                                    link.map,
                                    dir.opposite(),
                                    back.map
                                ),
                            });
                        }
                        Some(back) if back.offset != link.offset => {
                            return Err(WorldError::LayoutInvalid {
                                file: file.into(),
                                message: format!(
                                    "map `{map_id}` {:?} → `{}` offset {:?} does not match `{}` {:?} offset {:?}",
                                    dir,
                                    link.map,
                                    link.offset,
                                    link.map,
                                    dir.opposite(),
                                    back.offset
                                ),
                            });
                        }
                        Some(_) => {}
                    }
                    let _ = (from_size, to_size);
                }
            }
        }
        Ok(())
    }

    /// Map ids declared in this layout.
    pub fn map_ids(&self) -> impl Iterator<Item = &str> {
        self.maps.keys().map(String::as_str)
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
        Direction::Up => {
            let x = exit.x.checked_sub(ox)?;
            GridPos::new(x, max_y.checked_sub(oy)?)
        }
        Direction::Down => {
            let x = exit.x.checked_sub(ox)?;
            GridPos::new(x, oy)
        }
        Direction::Left => {
            let y = exit.y.checked_sub(oy)?;
            GridPos::new(max_x.checked_sub(ox)?, y)
        }
        Direction::Right => {
            let y = exit.y.checked_sub(oy)?;
            GridPos::new(ox, y)
        }
    };
    in_map(entry, to_size).then_some(entry)
}

fn cardinals(links: &MapLinks) -> [(Direction, Option<NeighborLink>); 4] {
    [
        (Direction::Up, links.north.clone()),
        (Direction::Down, links.south.clone()),
        (Direction::Left, links.west.clone()),
        (Direction::Right, links.east.clone()),
    ]
}

fn offset_along_edge_only(dir: Direction, offset: (i32, i32)) -> bool {
    let (ox, oy) = offset;
    match dir {
        Direction::Up | Direction::Down => oy == 0,
        Direction::Left | Direction::Right => ox == 0,
    }
}

fn sample_entry_in_map(dir: Direction, offset: (i32, i32), to_size: (u32, u32)) -> bool {
    let (w, h) = to_size;
    if w == 0 || h == 0 {
        return false;
    }
    let max_x = i32::try_from(w - 1).unwrap_or(i32::MAX);
    let max_y = i32::try_from(h - 1).unwrap_or(i32::MAX);
    let sample = match dir {
        Direction::Up => GridPos::new(max_x / 2, max_y),
        Direction::Down => GridPos::new(max_x / 2, 0),
        Direction::Left => GridPos::new(max_x, max_y / 2),
        Direction::Right => GridPos::new(0, max_y / 2),
    };
    translate_entry(sample, dir, offset, to_size).is_some()
}

fn validate_links_for_map(
    map_id: &str,
    links: &MapLinks,
    file: &str,
    map_sizes: &BTreeMap<String, (u32, u32)>,
) -> Result<bool> {
    for (dir, link) in cardinals(links) {
        let Some(link) = link else { continue };
        if !map_sizes.contains_key(&link.map) {
            return Err(WorldError::LayoutInvalid {
                file: file.into(),
                message: format!("map `{map_id}` {:?} → unknown map `{}`", dir, link.map),
            });
        }
    }
    Ok(true)
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
            GridPos::new(1, 7)
        );
    }

    #[test]
    fn entry_offset_matches_neighbor_placement() {
        let exit = GridPos::new(3, 0);
        let offset = (2, 0);
        let main_size = (20u32, 14);
        let neighbor_size = (20u32, 6);
        let entry = translate_entry(exit, Direction::Up, offset, neighbor_size).unwrap();
        let (ox_px, oy_px) =
            neighbor_pixel_offset(Direction::Up, main_size, neighbor_size, (16, 16), offset);
        let tw = 16f32;
        let main_px_x = exit.x as f32 * tw;
        let neighbor_entry_px_x = ox_px + entry.x as f32 * tw;
        assert!(
            (main_px_x - neighbor_entry_px_x).abs() < f32::EPSILON,
            "visual alignment: main x={main_px_x}, neighbour entry x={neighbor_entry_px_x}"
        );
        let _ = oy_px;
    }

    #[test]
    fn translate_entry_checked_on_overflow() {
        assert!(
            translate_entry(GridPos::new(0, 0), Direction::Up, (i32::MAX, 0), (10, 8)).is_none()
        );
    }

    #[test]
    fn validation_rejects_self_link() {
        let bad = r#"{ route_a: ( north: (map: "route_a") ) }"#;
        let err = WorldLayout::parse(bad, "world.ron").unwrap_err();
        assert!(matches!(err, WorldError::LayoutInvalid { .. }));
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
