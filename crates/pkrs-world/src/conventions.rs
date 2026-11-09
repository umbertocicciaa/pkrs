//! §6.2 layer conventions: turns a [`RawMap`] into a [`Map`], collecting every violation.
//!
//! | Layer | Kind | Meaning |
//! |---|---|---|
//! | `ground` (required) | tiles | below entities |
//! | `ground2`, `ground3`, … | tiles | ground decoration, below entities |
//! | `overhead` | tiles | above entities |
//! | `collision` | tiles | any tile is solid, unless it only sets `block_n/s/e/w` |
//! | `terrain` | tiles | logical only (not drawn), carries `terrain=` tiles |
//! | `objects` | objects | `warp`, `npc`, `trigger`, `sign`, `item`, `spawn` |
//! | `encounters` | objects | rects with `table` (string) and `rate` (int 0–100) |
//!
//! Tile properties honoured on **any** tile layer: `solid` (bool), `block_n/s/e/w` (bool) and
//! `terrain` (string). Later layers override the terrain of earlier ones.
//!
//! Tilesets and drawn tile layers may set `material` (string): the id of a material
//! descriptor `assets/materials/<id>.ron` (§5.6), kept in [`MapTileset::material`] and
//! [`TileLayer::material`].
//!
//! Violations are returned all at once (not fail-fast) so `pkrs check` can print them together.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use pkrs_core::{Direction, GridPos};

use crate::map::{
    EdgeMask, EncounterZone, LogicalGrid, Map, MapObject, MapTileset, ObjectKind, Properties,
    Terrain, TileLayer, TileRect, Transition, VisualLayerKind, Warp,
};
use crate::raw::{RawLayer, RawMap, RawObject, RawOrientation, RawTileset};

/// Where in the map a violation was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViolationLocation {
    /// The map as a whole.
    Map,
    /// A layer, by name.
    Layer(String),
    /// A tileset, by name.
    Tileset(String),
    /// One tile of a tileset.
    Tile {
        /// Tileset name.
        tileset: String,
        /// Local tile id.
        id: u32,
    },
    /// An object on an object layer.
    Object {
        /// Layer name.
        layer: String,
        /// Object id.
        id: u32,
        /// Object name (may be empty).
        name: String,
    },
}

impl fmt::Display for ViolationLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Map => write!(f, "map"),
            Self::Layer(name) => write!(f, "layer `{name}`"),
            Self::Tileset(name) => write!(f, "tileset `{name}`"),
            Self::Tile { tileset, id } => write!(f, "tile {id} of tileset `{tileset}`"),
            Self::Object { layer, id, name } if name.is_empty() => {
                write!(f, "object #{id} in layer `{layer}`")
            }
            Self::Object { layer, id, name } => {
                write!(f, "object #{id} `{name}` in layer `{layer}`")
            }
        }
    }
}

/// What rule a map breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ViolationKind {
    /// Only orthogonal maps are supported.
    UnsupportedOrientation,
    /// Infinite (chunked) maps are not supported.
    InfiniteMap,
    /// Width, height or tile size is zero.
    EmptyMap,
    /// A required layer is missing.
    MissingLayer(&'static str),
    /// Two layers share a name.
    DuplicateLayer,
    /// A layer name is not part of the conventions.
    UnknownLayer {
        /// `tile` or `object`.
        kind: &'static str,
    },
    /// A known layer name has the wrong layer kind.
    WrongLayerKind {
        /// Kind the convention requires.
        expected: &'static str,
        /// Kind found in the file.
        found: &'static str,
    },
    /// Image, group or other unsupported layer kind.
    UnsupportedLayer(&'static str),
    /// A tile layer's data length does not match the map size.
    LayerSizeMismatch,
    /// A tileset's tile size differs from the map's.
    TileSizeMismatch {
        /// Map tile size.
        expected: (u32, u32),
        /// Tileset tile size.
        found: (u32, u32),
    },
    /// The object has no class, or a class the conventions do not know.
    UnknownObjectClass(String),
    /// A required property is missing.
    MissingProperty(&'static str),
    /// A property has the wrong type.
    WrongPropertyType {
        /// Property key.
        key: String,
        /// Expected Tiled type.
        expected: &'static str,
        /// Type found.
        found: &'static str,
    },
    /// A property value is out of range or malformed.
    InvalidPropertyValue {
        /// Property key.
        key: String,
        /// Why it is invalid.
        message: String,
    },
    /// The object's origin lies outside the map.
    ObjectOutOfBounds,
}

impl fmt::Display for ViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOrientation => write!(f, "only orthogonal maps are supported"),
            Self::InfiniteMap => write!(f, "infinite maps are not supported"),
            Self::EmptyMap => write!(f, "map width, height and tile size must be non-zero"),
            Self::MissingLayer(name) => write!(f, "required layer `{name}` is missing"),
            Self::DuplicateLayer => write!(f, "layer name is used more than once"),
            Self::UnknownLayer { kind } => write!(
                f,
                "unknown {kind} layer (expected one of: ground, ground2…, overhead, collision, terrain, objects, encounters)"
            ),
            Self::WrongLayerKind { expected, found } => write!(
                f,
                "must be {} {expected} layer, found {} {found} layer",
                article(expected),
                article(found)
            ),
            Self::UnsupportedLayer(kind) => write!(f, "{kind}s are not supported"),
            Self::LayerSizeMismatch => write!(f, "layer size does not match the map size"),
            Self::TileSizeMismatch { expected, found } => write!(
                f,
                "tile size {}x{} does not match the map tile size {}x{}",
                found.0, found.1, expected.0, expected.1
            ),
            Self::UnknownObjectClass(class) if class.is_empty() => write!(
                f,
                "object has no class (expected warp, npc, trigger, sign, item or spawn)"
            ),
            Self::UnknownObjectClass(class) => write!(
                f,
                "unknown object class `{class}` (expected warp, npc, trigger, sign, item or spawn)"
            ),
            Self::MissingProperty(key) => write!(f, "missing required property `{key}`"),
            Self::WrongPropertyType {
                key,
                expected,
                found,
            } => write!(f, "property `{key}` must be {expected}, found {found}"),
            Self::InvalidPropertyValue { key, message } => {
                write!(f, "property `{key}`: {message}")
            }
            Self::ObjectOutOfBounds => write!(f, "object lies outside the map"),
        }
    }
}

fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// One broken convention, with its location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConventionViolation {
    /// Where it was found.
    pub location: ViolationLocation,
    /// What is wrong.
    pub kind: ViolationKind,
}

impl fmt::Display for ConventionViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.kind)
    }
}

/// Collects violations while building.
#[derive(Default)]
struct Report(Vec<ConventionViolation>);

impl Report {
    fn push(&mut self, location: ViolationLocation, kind: ViolationKind) {
        self.0.push(ConventionViolation { location, kind });
    }
}

/// Parsed per-tile logical rules (from tileset tile properties).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TileRule {
    solid: Option<bool>,
    edges: EdgeMask,
    terrain: Option<Terrain>,
}

const EDGE_KEYS: [(&str, Direction); 4] = [
    ("block_n", Direction::Up),
    ("block_s", Direction::Down),
    ("block_w", Direction::Left),
    ("block_e", Direction::Right),
];

/// Role of a tile layer under the conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TileRole {
    Visual(VisualLayerKind),
    Collision,
    Terrain,
}

fn tile_role(name: &str) -> Option<TileRole> {
    match name {
        "ground" => Some(TileRole::Visual(VisualLayerKind::Ground)),
        "overhead" => Some(TileRole::Visual(VisualLayerKind::Overhead)),
        "collision" => Some(TileRole::Collision),
        "terrain" => Some(TileRole::Terrain),
        _ => {
            let digits = name.strip_prefix("ground")?;
            let n: u32 = digits.parse().ok()?;
            (n >= 2 && !digits.starts_with('0'))
                .then_some(TileRole::Visual(VisualLayerKind::GroundDecor))
        }
    }
}

/// Builds a [`Map`] from `raw`, or returns every convention violation found.
pub fn build_map(raw: RawMap) -> Result<Map, Vec<ConventionViolation>> {
    let mut report = Report::default();

    if raw.orientation != RawOrientation::Orthogonal {
        report.push(
            ViolationLocation::Map,
            ViolationKind::UnsupportedOrientation,
        );
    }
    if raw.infinite {
        report.push(ViolationLocation::Map, ViolationKind::InfiniteMap);
    }
    if raw.width == 0 || raw.height == 0 || raw.tile_width == 0 || raw.tile_height == 0 {
        report.push(ViolationLocation::Map, ViolationKind::EmptyMap);
        return Err(report.0);
    }

    let rules = tile_rules(&raw, &mut report);
    let tilesets = raw
        .tilesets
        .iter()
        .map(|ts| to_map_tileset(ts, &mut report))
        .collect();

    let mut grid = LogicalGrid::new(raw.width, raw.height);
    let mut layers = Vec::new();
    let mut objects = Vec::new();
    let mut encounters = Vec::new();
    let mut seen = BTreeSet::new();
    let expected_len = (raw.width as usize).saturating_mul(raw.height as usize);
    let dims = raw_dims(&raw);

    for layer in raw.layers {
        let name = layer.name().to_owned();
        let location = || ViolationLocation::Layer(name.clone());
        if !seen.insert(name.clone()) {
            report.push(location(), ViolationKind::DuplicateLayer);
            continue;
        }
        match layer {
            RawLayer::Unsupported { kind, .. } => {
                report.push(location(), ViolationKind::UnsupportedLayer(kind));
            }
            RawLayer::Tiles {
                visible,
                tiles,
                properties,
                ..
            } => {
                if matches!(name.as_str(), "objects" | "encounters") {
                    report.push(
                        location(),
                        ViolationKind::WrongLayerKind {
                            expected: "object",
                            found: "tile",
                        },
                    );
                    continue;
                }
                let Some(role) = tile_role(&name) else {
                    report.push(location(), ViolationKind::UnknownLayer { kind: "tile" });
                    continue;
                };
                if tiles.len() != expected_len {
                    report.push(location(), ViolationKind::LayerSizeMismatch);
                    continue;
                }
                apply_logical(&mut grid, raw.width, &tiles, &rules, role);
                if let TileRole::Visual(kind) = role {
                    if let Some(mut tile_layer) =
                        TileLayer::new(name.clone(), kind, raw.width, raw.height, tiles)
                    {
                        tile_layer.visible = visible;
                        tile_layer.material =
                            material_property(&properties, location(), &mut report);
                        layers.push(tile_layer);
                    }
                }
            }
            RawLayer::Objects {
                objects: raw_objects,
                ..
            } => match name.as_str() {
                "objects" => {
                    for obj in &raw_objects {
                        if let Some(o) = build_object(&name, obj, &dims, &mut report) {
                            objects.push(o);
                        }
                    }
                }
                "encounters" => {
                    for obj in &raw_objects {
                        if let Some(z) = build_encounter(&name, obj, &dims, &mut report) {
                            encounters.push(z);
                        }
                    }
                }
                _ if tile_role(&name).is_some() => report.push(
                    location(),
                    ViolationKind::WrongLayerKind {
                        expected: "tile",
                        found: "object",
                    },
                ),
                _ => report.push(location(), ViolationKind::UnknownLayer { kind: "object" }),
            },
        }
    }

    if !seen.contains("ground") {
        report.push(
            ViolationLocation::Map,
            ViolationKind::MissingLayer("ground"),
        );
    }

    if !report.0.is_empty() {
        return Err(report.0);
    }
    Ok(Map {
        id: raw.id,
        width: raw.width,
        height: raw.height,
        tile_width: raw.tile_width,
        tile_height: raw.tile_height,
        properties: raw.properties,
        tilesets,
        layers,
        grid,
        objects,
        encounters,
    })
}

/// Map size and tile size, used to convert object pixels to tiles.
#[derive(Clone, Copy)]
struct Dims {
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
}

fn raw_dims(raw: &RawMap) -> Dims {
    Dims {
        width: raw.width,
        height: raw.height,
        tile_width: raw.tile_width,
        tile_height: raw.tile_height,
    }
}

fn to_map_tileset(ts: &RawTileset, report: &mut Report) -> MapTileset {
    MapTileset {
        name: ts.name.clone(),
        tile_width: ts.tile_width,
        tile_height: ts.tile_height,
        columns: ts.columns,
        tile_count: ts.tile_count,
        spacing: ts.spacing,
        margin: ts.margin,
        image: ts.image.clone(),
        material: material_property(
            &ts.properties,
            ViolationLocation::Tileset(ts.name.clone()),
            report,
        ),
    }
}

/// The `material` property (a non-empty material id, §5.6) of a tileset or layer.
fn material_property(
    props: &Properties,
    location: ViolationLocation,
    report: &mut Report,
) -> Option<String> {
    match props.string("material") {
        Ok(Some(id)) if !id.trim().is_empty() => Some(id.to_owned()),
        Ok(_) => None,
        Err(found) => {
            report.push(location, wrong_type("material", "string", found));
            None
        }
    }
}

fn tile_rules(raw: &RawMap, report: &mut Report) -> BTreeMap<(u16, u32), TileRule> {
    let mut rules = BTreeMap::new();
    for (index, ts) in raw.tilesets.iter().enumerate() {
        if (ts.tile_width, ts.tile_height) != (raw.tile_width, raw.tile_height) {
            report.push(
                ViolationLocation::Tileset(ts.name.clone()),
                ViolationKind::TileSizeMismatch {
                    expected: (raw.tile_width, raw.tile_height),
                    found: (ts.tile_width, ts.tile_height),
                },
            );
        }
        let Ok(index) = u16::try_from(index) else {
            continue;
        };
        for (&id, props) in &ts.tile_properties {
            let location = || ViolationLocation::Tile {
                tileset: ts.name.clone(),
                id,
            };
            let mut rule = TileRule::default();
            match props.bool("solid") {
                Ok(solid) => rule.solid = solid,
                Err(found) => report.push(location(), wrong_type("solid", "bool", found)),
            }
            for (key, dir) in EDGE_KEYS {
                match props.bool(key) {
                    Ok(Some(true)) => rule.edges = rule.edges.with(dir),
                    Ok(_) => {}
                    Err(found) => report.push(location(), wrong_type(key, "bool", found)),
                }
            }
            match props.string("terrain") {
                Ok(Some(raw)) => match Terrain::parse(raw) {
                    Ok(t) => rule.terrain = Some(t),
                    Err(message) => report.push(
                        location(),
                        ViolationKind::InvalidPropertyValue {
                            key: "terrain".into(),
                            message,
                        },
                    ),
                },
                Ok(None) => {}
                Err(found) => report.push(location(), wrong_type("terrain", "string", found)),
            }
            if rule != TileRule::default() {
                rules.insert((index, id), rule);
            }
        }
    }
    rules
}

fn apply_logical(
    grid: &mut LogicalGrid,
    width: u32,
    tiles: &[Option<crate::map::TileRef>],
    rules: &BTreeMap<(u16, u32), TileRule>,
    role: TileRole,
) {
    let width = width.max(1) as usize;
    for (i, tile) in tiles.iter().enumerate() {
        let Some(tile) = tile else { continue };
        let (Ok(x), Ok(y)) = (i32::try_from(i % width), i32::try_from(i / width)) else {
            continue;
        };
        let Some(cell) = grid.cell_mut(GridPos::new(x, y)) else {
            continue;
        };
        let rule = rules.get(&(tile.tileset, tile.id));
        let edges = rule.map_or(EdgeMask::NONE, |r| r.edges);
        let explicit_solid = rule.and_then(|r| r.solid);
        let solid = match role {
            // Collision tiles block fully unless they only declare edges (fences, one-way
            // walls) or explicitly say `solid=false`.
            TileRole::Collision => explicit_solid.unwrap_or(edges.is_empty()),
            TileRole::Visual(_) | TileRole::Terrain => explicit_solid.unwrap_or(false),
        };
        cell.solid |= solid;
        cell.blocked_edges = cell.blocked_edges.union(edges);
        if let Some(terrain) = rule.and_then(|r| r.terrain.clone()) {
            cell.terrain = Some(terrain);
        }
    }
}

fn wrong_type(key: &str, expected: &'static str, found: &'static str) -> ViolationKind {
    ViolationKind::WrongPropertyType {
        key: key.to_owned(),
        expected,
        found,
    }
}

/// Converts an object's pixel rect to tiles; `None` (with a violation) if out of bounds.
fn object_rect(
    obj: &RawObject,
    dims: &Dims,
    location: &ViolationLocation,
    report: &mut Report,
) -> Option<TileRect> {
    let tw = dims.tile_width as f32;
    let th = dims.tile_height as f32;
    let x = (obj.x / tw).floor();
    let y = (obj.y / th).floor();
    let w = (obj.width / tw).ceil().max(1.0);
    let h = (obj.height / th).ceil().max(1.0);
    let in_bounds = x >= 0.0 && y >= 0.0 && x < dims.width as f32 && y < dims.height as f32;
    if !in_bounds || !x.is_finite() || !y.is_finite() {
        report.push(location.clone(), ViolationKind::ObjectOutOfBounds);
        return None;
    }
    // In-bounds values fit in i32/u32 (map sizes are u32 tile counts).
    Some(TileRect::new(x as i32, y as i32, w as u32, h as u32))
}

/// Typed property readers that record violations for the current object.
struct PropReader<'a> {
    props: &'a Properties,
    location: ViolationLocation,
    report: &'a mut Report,
    ok: bool,
}

impl PropReader<'_> {
    fn fail(&mut self, kind: ViolationKind) {
        self.ok = false;
        self.report.push(self.location.clone(), kind);
    }

    fn opt_string(&mut self, key: &str) -> Option<String> {
        match self.props.string(key) {
            Ok(v) => v.filter(|s| !s.is_empty()).map(str::to_owned),
            Err(found) => {
                self.fail(wrong_type(key, "string", found));
                None
            }
        }
    }

    fn req_string(&mut self, key: &'static str) -> Option<String> {
        if !self.props.contains(key) {
            self.fail(ViolationKind::MissingProperty(key));
            return None;
        }
        let value = self.opt_string(key);
        if value.is_none() && self.ok {
            self.fail(ViolationKind::InvalidPropertyValue {
                key: key.into(),
                message: "must not be empty".into(),
            });
        }
        value
    }

    fn opt_int(&mut self, key: &str) -> Option<i64> {
        match self.props.int(key) {
            Ok(v) => v,
            Err(found) => {
                self.fail(wrong_type(key, "int", found));
                None
            }
        }
    }

    fn req_int(&mut self, key: &'static str) -> Option<i64> {
        if !self.props.contains(key) {
            self.fail(ViolationKind::MissingProperty(key));
            return None;
        }
        self.opt_int(key)
    }

    fn int_in(&mut self, key: &'static str, value: i64, min: i64, max: i64) -> Option<i64> {
        if (min..=max).contains(&value) {
            Some(value)
        } else {
            self.fail(ViolationKind::InvalidPropertyValue {
                key: key.into(),
                message: format!("must be in {min}..={max}, found {value}"),
            });
            None
        }
    }

    fn opt_bool(&mut self, key: &str) -> Option<bool> {
        match self.props.bool(key) {
            Ok(v) => v,
            Err(found) => {
                self.fail(wrong_type(key, "bool", found));
                None
            }
        }
    }

    fn opt_facing(&mut self, key: &str) -> Option<Direction> {
        let raw = self.opt_string(key)?;
        match raw.parse::<Direction>() {
            Ok(dir) => Some(dir),
            Err(_) => {
                self.fail(ViolationKind::InvalidPropertyValue {
                    key: key.into(),
                    message: format!("`{raw}` is not one of up, down, left, right"),
                });
                None
            }
        }
    }
}

fn build_object(
    layer: &str,
    obj: &RawObject,
    dims: &Dims,
    report: &mut Report,
) -> Option<MapObject> {
    let location = ViolationLocation::Object {
        layer: layer.to_owned(),
        id: obj.id,
        name: obj.name.clone(),
    };
    let rect = object_rect(obj, dims, &location, report);
    let mut r = PropReader {
        props: &obj.properties,
        location,
        report,
        ok: true,
    };
    let kind = match obj.class.as_str() {
        "warp" => {
            let to_map = r.req_string("to_map");
            let to_x = r
                .req_int("to_x")
                .and_then(|v| r.int_in("to_x", v, 0, i64::from(i32::MAX)));
            let to_y = r
                .req_int("to_y")
                .and_then(|v| r.int_in("to_y", v, 0, i64::from(i32::MAX)));
            let facing = r.opt_facing("facing");
            let transition = match r.opt_string("transition") {
                None => Transition::default(),
                Some(raw) => Transition::parse(&raw).unwrap_or_else(|| {
                    r.fail(ViolationKind::InvalidPropertyValue {
                        key: "transition".into(),
                        message: format!("`{raw}` is not one of fade, none, door"),
                    });
                    Transition::default()
                }),
            };
            match (to_map, to_x, to_y) {
                (Some(to_map), Some(x), Some(y)) => ObjectKind::Warp(Warp {
                    to_map,
                    to: GridPos::new(i32::try_from(x).ok()?, i32::try_from(y).ok()?),
                    facing,
                    transition,
                }),
                _ => return None,
            }
        }
        "npc" => ObjectKind::Npc {
            script: r.opt_string("script"),
            sprite: r.opt_string("sprite"),
            facing: r.opt_facing("facing").unwrap_or(Direction::Down),
        },
        "trigger" => ObjectKind::Trigger {
            on_enter: r.opt_string("on_enter"),
            once: r.opt_bool("once").unwrap_or(false),
        },
        "sign" => ObjectKind::Sign {
            script: r.opt_string("script"),
            text: r.opt_string("text"),
        },
        "item" => {
            let item = r.req_string("item");
            let count = match r.opt_int("count") {
                None => Some(1),
                Some(v) => r.int_in("count", v, 1, i64::from(u32::MAX)),
            };
            ObjectKind::Item {
                item: item?,
                count: u32::try_from(count?).ok()?,
            }
        }
        "spawn" => ObjectKind::Spawn {
            facing: r.opt_facing("facing"),
        },
        other => {
            r.fail(ViolationKind::UnknownObjectClass(other.to_owned()));
            return None;
        }
    };
    if !r.ok {
        return None;
    }
    Some(MapObject {
        id: obj.id,
        name: obj.name.clone(),
        rect: rect?,
        kind,
        properties: obj.properties.clone(),
    })
}

fn build_encounter(
    layer: &str,
    obj: &RawObject,
    dims: &Dims,
    report: &mut Report,
) -> Option<EncounterZone> {
    let location = ViolationLocation::Object {
        layer: layer.to_owned(),
        id: obj.id,
        name: obj.name.clone(),
    };
    let rect = object_rect(obj, dims, &location, report);
    let mut r = PropReader {
        props: &obj.properties,
        location,
        report,
        ok: true,
    };
    let table = r.req_string("table");
    let rate = r.req_int("rate").and_then(|v| r.int_in("rate", v, 0, 100));
    Some(EncounterZone {
        id: obj.id,
        rect: rect?,
        table: table?,
        rate: u8::try_from(rate?).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{PropertyValue, TileRef};

    const W: u32 = 4;
    const H: u32 = 3;

    fn tileset(props: &[(u32, Properties)]) -> RawTileset {
        RawTileset {
            name: "basic".into(),
            tile_width: 16,
            tile_height: 16,
            columns: 4,
            tile_count: 16,
            spacing: 0,
            margin: 0,
            image: None,
            tile_properties: props.iter().cloned().collect(),
            properties: Properties::new(),
        }
    }

    fn tiles_with(entries: &[((u32, u32), u32)]) -> Vec<Option<TileRef>> {
        let mut tiles = vec![None; (W * H) as usize];
        for &((x, y), id) in entries {
            tiles[(y * W + x) as usize] = Some(TileRef::new(0, id));
        }
        tiles
    }

    fn tile_layer(name: &str, tiles: Vec<Option<TileRef>>) -> RawLayer {
        RawLayer::Tiles {
            name: name.into(),
            visible: true,
            tiles,
            properties: Properties::new(),
        }
    }

    fn object(id: u32, class: &str, tx: u32, ty: u32, props: Properties) -> RawObject {
        RawObject {
            id,
            name: format!("{class}{id}"),
            class: class.into(),
            x: (tx * 16) as f32,
            y: (ty * 16) as f32,
            width: 16.0,
            height: 16.0,
            properties: props,
        }
    }

    fn raw(layers: Vec<RawLayer>, tileset_props: &[(u32, Properties)]) -> RawMap {
        RawMap {
            id: "test".into(),
            orientation: RawOrientation::Orthogonal,
            infinite: false,
            width: W,
            height: H,
            tile_width: 16,
            tile_height: 16,
            properties: Properties::new(),
            tilesets: vec![tileset(tileset_props)],
            layers,
        }
    }

    fn ground() -> RawLayer {
        tile_layer("ground", tiles_with(&[((0, 0), 0)]))
    }

    fn kinds(violations: &[ConventionViolation]) -> Vec<&ViolationKind> {
        violations.iter().map(|v| &v.kind).collect()
    }

    #[test]
    fn minimal_map_builds() {
        let map = build_map(raw(vec![ground()], &[])).unwrap();
        assert_eq!(map.layers.len(), 1);
        assert_eq!(map.layers[0].kind, VisualLayerKind::Ground);
        assert!(!map.grid.is_solid(GridPos::new(0, 0)));
    }

    #[test]
    fn layer_names_map_to_visual_kinds() {
        let empty = || vec![None; (W * H) as usize];
        let map = build_map(raw(
            vec![
                ground(),
                tile_layer("ground2", empty()),
                tile_layer("ground3", empty()),
                tile_layer("overhead", empty()),
                tile_layer("collision", empty()),
                tile_layer("terrain", empty()),
            ],
            &[],
        ))
        .unwrap();
        let kinds: Vec<_> = map
            .layers
            .iter()
            .map(|l| (l.name.as_str(), l.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("ground", VisualLayerKind::Ground),
                ("ground2", VisualLayerKind::GroundDecor),
                ("ground3", VisualLayerKind::GroundDecor),
                ("overhead", VisualLayerKind::Overhead),
            ],
            "collision and terrain are logical only"
        );
    }

    #[test]
    fn material_properties_on_tilesets_and_layers() {
        let mut raw_map = raw(vec![ground()], &[]);
        raw_map.tilesets[0].properties =
            Properties::new().with("material", PropertyValue::String("water".into()));
        if let RawLayer::Tiles { properties, .. } = &mut raw_map.layers[0] {
            *properties =
                Properties::new().with("material", PropertyValue::String("day_night".into()));
        }
        let map = build_map(raw_map).unwrap();
        assert_eq!(map.tilesets[0].material.as_deref(), Some("water"));
        assert_eq!(map.layers[0].material.as_deref(), Some("day_night"));

        let mut bad = raw(vec![ground()], &[]);
        bad.tilesets[0].properties = Properties::new().with("material", PropertyValue::Int(3));
        let err = build_map(bad).unwrap_err();
        assert!(matches!(
            kinds(&err)[..],
            [ViolationKind::WrongPropertyType { key, .. }] if key == "material"
        ));
    }

    #[test]
    fn collision_layer_tiles_are_solid() {
        let map = build_map(raw(
            vec![
                ground(),
                tile_layer("collision", tiles_with(&[((2, 1), 9)])),
            ],
            &[],
        ))
        .unwrap();
        assert!(map.grid.is_solid(GridPos::new(2, 1)));
        assert!(!map.grid.is_solid(GridPos::new(1, 1)));
    }

    #[test]
    fn solid_property_works_on_any_layer() {
        let props = Properties::new().with("solid", PropertyValue::Bool(true));
        let map = build_map(raw(
            vec![tile_layer("ground", tiles_with(&[((1, 0), 3)]))],
            &[(3, props)],
        ))
        .unwrap();
        assert!(map.grid.is_solid(GridPos::new(1, 0)));
    }

    #[test]
    fn directional_blocking_from_tile_properties() {
        let fence = Properties::new().with("block_s", PropertyValue::Bool(true));
        let map = build_map(raw(
            vec![
                ground(),
                tile_layer("collision", tiles_with(&[((1, 1), 4)])),
            ],
            &[(4, fence)],
        ))
        .unwrap();
        let fence_tile = GridPos::new(1, 1);
        assert!(
            !map.grid.is_solid(fence_tile),
            "edge-only tiles stay walkable"
        );
        assert!(!map.grid.can_cross(fence_tile, Direction::Down));
        assert!(!map.grid.can_cross(GridPos::new(1, 2), Direction::Up));
        assert!(map.grid.can_cross(fence_tile, Direction::Right));
    }

    #[test]
    fn terrain_mapping_later_layers_override() {
        let grass = Properties::new().with("terrain", PropertyValue::String("tall_grass".into()));
        let ledge = Properties::new().with("terrain", PropertyValue::String("ledge_s".into()));
        let map = build_map(raw(
            vec![
                tile_layer("ground", tiles_with(&[((0, 0), 1), ((1, 0), 1)])),
                tile_layer("terrain", tiles_with(&[((1, 0), 2)])),
            ],
            &[(1, grass), (2, ledge)],
        ))
        .unwrap();
        assert_eq!(
            map.grid.terrain(GridPos::new(0, 0)),
            Some(&Terrain::TallGrass)
        );
        assert_eq!(
            map.grid.terrain(GridPos::new(1, 0)),
            Some(&Terrain::Ledge(Direction::Down))
        );
        assert_eq!(map.grid.terrain(GridPos::new(2, 0)), None);
    }

    #[test]
    fn property_type_errors_are_reported_per_tile() {
        let bad = Properties::new()
            .with("solid", PropertyValue::String("yes".into()))
            .with("terrain", PropertyValue::String("ledge_q".into()));
        let errs = build_map(raw(vec![ground()], &[(5, bad)])).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().all(|e| e.location
            == ViolationLocation::Tile {
                tileset: "basic".into(),
                id: 5
            }));
        assert!(matches!(
            errs[0].kind,
            ViolationKind::WrongPropertyType { .. }
        ));
        assert!(matches!(
            errs[1].kind,
            ViolationKind::InvalidPropertyValue { .. }
        ));
    }

    #[test]
    fn all_object_kinds_are_typed() {
        let objects = vec![
            object(
                1,
                "warp",
                0,
                0,
                Properties::new()
                    .with("to_map", PropertyValue::String("house".into()))
                    .with("to_x", PropertyValue::Int(3))
                    .with("to_y", PropertyValue::Int(4))
                    .with("facing", PropertyValue::String("up".into()))
                    .with("transition", PropertyValue::String("door".into())),
            ),
            object(
                2,
                "npc",
                1,
                0,
                Properties::new().with("script", PropertyValue::String("maps/t.lua:hi".into())),
            ),
            object(
                3,
                "trigger",
                2,
                0,
                Properties::new()
                    .with("on_enter", PropertyValue::String("cutscenes/intro".into()))
                    .with("once", PropertyValue::Bool(true)),
            ),
            object(
                4,
                "sign",
                3,
                0,
                Properties::new().with("text", PropertyValue::String("text:sign.a".into())),
            ),
            object(
                5,
                "item",
                0,
                1,
                Properties::new().with("item", PropertyValue::String("potion".into())),
            ),
            object(6, "spawn", 1, 1, Properties::new()),
        ];
        let map = build_map(raw(
            vec![
                ground(),
                RawLayer::Objects {
                    name: "objects".into(),
                    objects,
                },
            ],
            &[],
        ))
        .unwrap();
        let classes: Vec<_> = map.objects.iter().map(|o| o.kind.class_name()).collect();
        assert_eq!(classes, ["warp", "npc", "trigger", "sign", "item", "spawn"]);
        assert_eq!(
            map.objects[0].kind,
            ObjectKind::Warp(Warp {
                to_map: "house".into(),
                to: GridPos::new(3, 4),
                facing: Some(Direction::Up),
                transition: Transition::Door,
            })
        );
        assert_eq!(
            map.objects[1].kind,
            ObjectKind::Npc {
                script: Some("maps/t.lua:hi".into()),
                sprite: None,
                facing: Direction::Down
            }
        );
        assert_eq!(
            map.objects[4].kind,
            ObjectKind::Item {
                item: "potion".into(),
                count: 1
            }
        );
        assert_eq!(map.objects[2].pos(), GridPos::new(2, 0));
        assert!(map.spawn("spawn6").is_some());
    }

    #[test]
    fn object_convention_errors_are_all_collected() {
        let objects = vec![
            object(1, "warp", 0, 0, Properties::new()),
            object(2, "dragon", 1, 0, Properties::new()),
            object(
                3,
                "npc",
                1,
                1,
                Properties::new().with("facing", PropertyValue::String("north".into())),
            ),
            object(4, "spawn", 9, 9, Properties::new()),
        ];
        let errs = build_map(raw(
            vec![
                ground(),
                RawLayer::Objects {
                    name: "objects".into(),
                    objects,
                },
            ],
            &[],
        ))
        .unwrap_err();
        assert_eq!(
            kinds(&errs),
            [
                &ViolationKind::MissingProperty("to_map"),
                &ViolationKind::MissingProperty("to_x"),
                &ViolationKind::MissingProperty("to_y"),
                &ViolationKind::UnknownObjectClass("dragon".into()),
                &ViolationKind::InvalidPropertyValue {
                    key: "facing".into(),
                    message: "`north` is not one of up, down, left, right".into()
                },
                &ViolationKind::ObjectOutOfBounds,
            ]
        );
    }

    #[test]
    fn encounter_rects_need_table_and_rate() {
        let mut zone = object(
            1,
            "",
            0,
            0,
            Properties::new()
                .with("table", PropertyValue::String("route1_grass".into()))
                .with("rate", PropertyValue::Int(20)),
        );
        zone.width = 32.0;
        let bad = object(
            2,
            "",
            0,
            1,
            Properties::new().with("rate", PropertyValue::Int(101)),
        );
        let errs = build_map(raw(
            vec![
                ground(),
                RawLayer::Objects {
                    name: "encounters".into(),
                    objects: vec![zone.clone(), bad],
                },
            ],
            &[],
        ))
        .unwrap_err();
        assert_eq!(
            kinds(&errs),
            [
                &ViolationKind::MissingProperty("table"),
                &ViolationKind::InvalidPropertyValue {
                    key: "rate".into(),
                    message: "must be in 0..=100, found 101".into()
                },
            ]
        );

        let map = build_map(raw(
            vec![
                ground(),
                RawLayer::Objects {
                    name: "encounters".into(),
                    objects: vec![zone],
                },
            ],
            &[],
        ))
        .unwrap();
        assert_eq!(map.encounters[0].rect, TileRect::new(0, 0, 2, 1));
        assert_eq!(map.encounters_at(GridPos::new(1, 0)).count(), 1);
        assert_eq!(map.encounters_at(GridPos::new(2, 0)).count(), 0);
    }

    #[test]
    fn layer_convention_errors() {
        let empty = || vec![None; (W * H) as usize];
        let errs = build_map(raw(
            vec![
                tile_layer("grass", empty()),
                tile_layer("ground1", empty()),
                tile_layer("objects", empty()),
                RawLayer::Objects {
                    name: "overhead".into(),
                    objects: vec![],
                },
                RawLayer::Unsupported {
                    name: "sky".into(),
                    kind: "image layer",
                },
                tile_layer("collision", vec![None; 2]),
                tile_layer("collision", empty()),
            ],
            &[],
        ))
        .unwrap_err();
        let rendered: Vec<String> = errs.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            [
                "layer `grass`: unknown tile layer (expected one of: ground, ground2…, overhead, collision, terrain, objects, encounters)",
                "layer `ground1`: unknown tile layer (expected one of: ground, ground2…, overhead, collision, terrain, objects, encounters)",
                "layer `objects`: must be an object layer, found a tile layer",
                "layer `overhead`: must be a tile layer, found an object layer",
                "layer `sky`: image layers are not supported",
                "layer `collision`: layer size does not match the map size",
                "layer `collision`: layer name is used more than once",
                "map: required layer `ground` is missing",
            ]
        );
    }

    #[test]
    fn map_level_errors() {
        let mut bad = raw(vec![ground()], &[]);
        bad.orientation = RawOrientation::Other;
        bad.infinite = true;
        bad.tilesets[0].tile_width = 8;
        let errs = build_map(bad).unwrap_err();
        assert_eq!(
            kinds(&errs),
            [
                &ViolationKind::UnsupportedOrientation,
                &ViolationKind::InfiniteMap,
                &ViolationKind::TileSizeMismatch {
                    expected: (16, 16),
                    found: (8, 16)
                },
            ]
        );

        let mut empty = raw(vec![ground()], &[]);
        empty.width = 0;
        assert_eq!(
            kinds(&build_map(empty).unwrap_err()),
            [&ViolationKind::EmptyMap]
        );
    }
}
