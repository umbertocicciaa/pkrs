//! Editor-neutral intermediate map produced by importers (§1.4, §6.1).
//!
//! A [`MapImporter`](crate::MapImporter) adapter translates its file format into a [`RawMap`]
//! (plain engine types, no vendor types), then [`conventions::build_map`](crate::conventions::build_map)
//! applies the §6.2 layer conventions. A future pkrs map editor only has to produce a `RawMap`.

use std::collections::BTreeMap;

use crate::map::{Properties, TileRef, TilesetImage};

/// Map projection; only orthogonal maps are supported in v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawOrientation {
    /// Square grid.
    Orthogonal,
    /// Any other projection (isometric, staggered, hexagonal).
    Other,
}

/// A tileset before convention checks.
#[derive(Clone, Debug, PartialEq)]
pub struct RawTileset {
    /// Tileset name.
    pub name: String,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// Columns in the image.
    pub columns: u32,
    /// Number of tiles.
    pub tile_count: u32,
    /// Pixels between tiles.
    pub spacing: u32,
    /// Pixels around the image border.
    pub margin: u32,
    /// Sheet image, if the tileset is a single image.
    pub image: Option<TilesetImage>,
    /// Custom properties per local tile id (`solid`, `terrain`, `block_n`, …).
    pub tile_properties: BTreeMap<u32, Properties>,
}

/// One object before convention checks (pixel coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct RawObject {
    /// Object id.
    pub id: u32,
    /// Object name.
    pub name: String,
    /// Class (Tiled ≥ 1.9) or type.
    pub class: String,
    /// Left edge in pixels.
    pub x: f32,
    /// Top edge in pixels.
    pub y: f32,
    /// Width in pixels (0 for points).
    pub width: f32,
    /// Height in pixels (0 for points).
    pub height: f32,
    /// Custom properties.
    pub properties: Properties,
}

/// One layer before convention checks.
#[derive(Clone, Debug, PartialEq)]
pub enum RawLayer {
    /// A finite tile layer with row-major tiles.
    Tiles {
        /// Layer name.
        name: String,
        /// Editor visibility.
        visible: bool,
        /// Row-major tiles (`width * height`).
        tiles: Vec<Option<TileRef>>,
    },
    /// An object layer.
    Objects {
        /// Layer name.
        name: String,
        /// Objects in file order.
        objects: Vec<RawObject>,
    },
    /// A layer kind the engine does not support (image, group, infinite tiles…).
    Unsupported {
        /// Layer name.
        name: String,
        /// Kind for the error message (`image layer`, `group layer`).
        kind: &'static str,
    },
}

impl RawLayer {
    /// Layer name.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Tiles { name, .. }
            | Self::Objects { name, .. }
            | Self::Unsupported { name, .. } => name,
        }
    }
}

/// A whole map before convention checks.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMap {
    /// Map id (file stem).
    pub id: String,
    /// Projection.
    pub orientation: RawOrientation,
    /// `true` for infinite (chunked) maps, which v1 rejects.
    pub infinite: bool,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// Map properties.
    pub properties: Properties,
    /// Tilesets (indexed by [`TileRef::tileset`]).
    pub tilesets: Vec<RawTileset>,
    /// Layers in file order (groups already flattened or reported as unsupported).
    pub layers: Vec<RawLayer>,
}
