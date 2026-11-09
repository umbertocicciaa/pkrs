//! Engine-owned map model (§6.1–§6.2).
//!
//! A [`Map`] keeps the **visual** tile layers (what the renderer draws) separate from the
//! **logical** [`LogicalGrid`] (collision, directional blocking, terrain) and the typed
//! [`MapObject`]s and [`EncounterZone`]s. Nothing here references the `tiled` crate: maps are
//! built by a [`MapImporter`](crate::MapImporter) adapter.

mod grid;
mod object;
mod properties;

pub use grid::{Cell, EdgeMask, LogicalGrid, Terrain};
pub use object::{EncounterZone, MapObject, ObjectKind, TileRect, Transition, Warp};
pub use properties::{Properties, PropertyValue};

use pkrs_core::GridPos;

/// Which band a visual tile layer draws in (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisualLayerKind {
    /// The `ground` layer.
    Ground,
    /// `ground2`, `ground3`, …: still below entities, above `ground`.
    GroundDecor,
    /// `overhead`: drawn above entities (tree tops, roofs).
    Overhead,
}

/// One placed tile: which tileset, which local tile id, and its flip flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileRef {
    /// Index into [`Map::tilesets`].
    pub tileset: u16,
    /// Tile id local to that tileset.
    pub id: u32,
    /// Mirrored horizontally.
    pub flip_h: bool,
    /// Mirrored vertically.
    pub flip_v: bool,
    /// Mirrored diagonally (rotation).
    pub flip_d: bool,
}

impl TileRef {
    /// Unflipped tile `id` of tileset `tileset`.
    #[must_use]
    pub const fn new(tileset: u16, id: u32) -> Self {
        Self {
            tileset,
            id,
            flip_h: false,
            flip_v: false,
            flip_d: false,
        }
    }
}

/// A drawable tile layer.
#[derive(Clone, Debug, PartialEq)]
pub struct TileLayer {
    /// Layer name as written in the map (`ground`, `ground2`, `overhead`).
    pub name: String,
    /// Draw band.
    pub kind: VisualLayerKind,
    /// Hidden in the editor: kept, but the renderer skips it.
    pub visible: bool,
    /// Material id from the layer property `material` (`assets/materials/<id>.ron`, §5.6).
    pub material: Option<String>,
    width: u32,
    height: u32,
    tiles: Vec<Option<TileRef>>,
}

impl TileLayer {
    /// Creates a layer from row-major `tiles` (`len` must be `width * height`).
    pub fn new(
        name: impl Into<String>,
        kind: VisualLayerKind,
        width: u32,
        height: u32,
        tiles: Vec<Option<TileRef>>,
    ) -> Option<Self> {
        let expected = (width as usize).checked_mul(height as usize)?;
        (tiles.len() == expected).then(|| Self {
            name: name.into(),
            kind,
            visible: true,
            material: None,
            width,
            height,
            tiles,
        })
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

    /// Tile at `(x, y)` (`None` when empty or out of bounds).
    #[must_use]
    pub fn tile(&self, x: u32, y: u32) -> Option<TileRef> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize) * (self.width as usize) + x as usize;
        self.tiles.get(i).copied().flatten()
    }
}

/// A tileset image referenced by a tileset (not decoded here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TilesetImage {
    /// Normalised project-relative path (`tilesets/town.png`).
    pub path: String,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
}

/// Tileset metadata needed to draw and to look up per-tile properties.
#[derive(Clone, Debug, PartialEq)]
pub struct MapTileset {
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
    /// Sheet image (single-image tilesets only in v1).
    pub image: Option<TilesetImage>,
    /// Material id from the tileset property `material` (`assets/materials/<id>.ron`, §5.6).
    pub material: Option<String>,
}

impl MapTileset {
    /// Source rectangle `(x, y, w, h)` of local tile `id` in the image, in pixels.
    #[must_use]
    pub fn tile_src(&self, id: u32) -> Option<(u32, u32, u32, u32)> {
        if self.columns == 0 || id >= self.tile_count {
            return None;
        }
        let col = id % self.columns;
        let row = id / self.columns;
        let x = self.margin + col * (self.tile_width + self.spacing);
        let y = self.margin + row * (self.tile_height + self.spacing);
        Some((x, y, self.tile_width, self.tile_height))
    }
}

/// A loaded map.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    /// Content id (the file stem, e.g. `route1`).
    pub id: String,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// Map-level custom properties (`bgm`, `script`, …).
    pub properties: Properties,
    /// Tilesets referenced by [`TileRef::tileset`].
    pub tilesets: Vec<MapTileset>,
    /// Visual layers in file order (`ground` first).
    pub layers: Vec<TileLayer>,
    /// Collision, directional blocking and terrain.
    pub grid: LogicalGrid,
    /// Typed objects from the `objects` layer.
    pub objects: Vec<MapObject>,
    /// Encounter rects from the `encounters` layer.
    pub encounters: Vec<EncounterZone>,
}

impl Map {
    /// Visual layers of `kind`, in draw order.
    pub fn layers_of(&self, kind: VisualLayerKind) -> impl Iterator<Item = &TileLayer> {
        self.layers.iter().filter(move |l| l.kind == kind)
    }

    /// Warp whose [`TileRect`](TileRect) contains `pos`, if any (§6.4).
    #[must_use]
    pub fn warp_at(&self, pos: GridPos) -> Option<&Warp> {
        self.objects.iter().find_map(|o| match &o.kind {
            ObjectKind::Warp(w) if o.rect.contains(pos) => Some(w),
            _ => None,
        })
    }

    /// Object with Tiled id `id`.
    #[must_use]
    pub fn object(&self, id: u32) -> Option<&MapObject> {
        self.objects.iter().find(|o| o.id == id)
    }

    /// Spawn point named `name` (objects of kind `spawn`).
    #[must_use]
    pub fn spawn(&self, name: &str) -> Option<&MapObject> {
        self.objects
            .iter()
            .find(|o| matches!(o.kind, ObjectKind::Spawn { .. }) && o.name == name)
    }

    /// Encounter zones covering `pos`.
    pub fn encounters_at(&self, pos: GridPos) -> impl Iterator<Item = &EncounterZone> {
        self.encounters.iter().filter(move |z| z.rect.contains(pos))
    }

    /// Map size in pixels.
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        (
            self.width.saturating_mul(self.tile_width),
            self.height.saturating_mul(self.tile_height),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_map(w: u32, h: u32) -> Map {
        let full = |id| vec![Some(TileRef::new(0, id)); (w * h) as usize];
        Map {
            id: "m".into(),
            width: w,
            height: h,
            tile_width: 16,
            tile_height: 16,
            properties: Properties::new(),
            tilesets: vec![MapTileset {
                name: "t".into(),
                tile_width: 16,
                tile_height: 16,
                columns: 4,
                tile_count: 16,
                spacing: 0,
                margin: 0,
                image: None,
                material: None,
            }],
            layers: vec![
                TileLayer::new("ground", VisualLayerKind::Ground, w, h, full(0)).unwrap(),
                TileLayer::new("overhead", VisualLayerKind::Overhead, w, h, full(1)).unwrap(),
            ],
            grid: LogicalGrid::new(w, h),
            objects: vec![],
            encounters: Vec::<EncounterZone>::new(),
        }
    }

    #[test]
    fn tile_layer_rejects_wrong_length() {
        assert!(TileLayer::new("ground", VisualLayerKind::Ground, 2, 2, vec![None; 3]).is_none());
        let layer = TileLayer::new(
            "ground",
            VisualLayerKind::Ground,
            2,
            1,
            vec![None, Some(TileRef::new(0, 5))],
        )
        .unwrap();
        assert_eq!(layer.tile(1, 0), Some(TileRef::new(0, 5)));
        assert_eq!(layer.tile(0, 0), None);
        assert_eq!(layer.tile(2, 0), None);
    }

    #[test]
    fn warp_at_finds_covering_warp() {
        let mut m = minimal_map(10, 10);
        m.objects.push(MapObject {
            id: 2,
            name: "door".into(),
            rect: TileRect::new(6, 10, 1, 1),
            kind: ObjectKind::Warp(Warp {
                to_map: "dest".into(),
                to: GridPos::new(5, 7),
                facing: None,
                transition: Transition::Fade,
            }),
            properties: Properties::new(),
        });
        assert!(m.warp_at(GridPos::new(6, 10)).is_some());
        assert!(m.warp_at(GridPos::new(5, 10)).is_none());
    }

    #[test]
    fn tileset_src_rect_honours_margin_and_spacing() {
        let ts = MapTileset {
            name: "t".into(),
            tile_width: 16,
            tile_height: 16,
            columns: 4,
            tile_count: 8,
            spacing: 1,
            margin: 2,
            image: None,
            material: None,
        };
        assert_eq!(ts.tile_src(0), Some((2, 2, 16, 16)));
        assert_eq!(ts.tile_src(5), Some((2 + 17, 2 + 17, 16, 16)));
        assert_eq!(ts.tile_src(8), None);
    }
}
