//! Tile layers and y-sorted entities (§5.2).
//!
//! `pkrs-render` does not know the world map model (§4.3). Callers expose a layer through the
//! [`TileGrid`] trait and the tileset images as [`TileSheet`]s; the renderer culls to the view
//! and queues one [`SpriteDraw`] per visible tile.

use core::cmp::Ordering;
use core::ops::Range;

use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::layer::Layer;
use crate::rect::Rect;
use crate::texture::TextureHandle;

/// A tileset image uploaded as a texture, with its grid layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSheet {
    /// Uploaded sheet.
    pub texture: TextureHandle,
    /// Tile width in pixels.
    pub tile_width: u32,
    /// Tile height in pixels.
    pub tile_height: u32,
    /// Tiles per row.
    pub columns: u32,
    /// Number of tiles.
    pub tile_count: u32,
    /// Border around the image in pixels.
    pub margin: u32,
    /// Gap between tiles in pixels.
    pub spacing: u32,
}

impl TileSheet {
    /// Source rect of local tile `id` (`None` if out of range).
    #[must_use]
    pub fn src(&self, id: u32) -> Option<Rect> {
        if self.columns == 0 || id >= self.tile_count {
            return None;
        }
        let col = id % self.columns;
        let row = id / self.columns;
        let x = self.margin + col * (self.tile_width + self.spacing);
        let y = self.margin + row * (self.tile_height + self.spacing);
        Some(Rect::new(
            x as f32,
            y as f32,
            self.tile_width as f32,
            self.tile_height as f32,
        ))
    }
}

/// One tile placed in a [`TileGrid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileInstance {
    /// Index into the `sheets` slice passed to the draw call.
    pub sheet: u16,
    /// Local tile id within the sheet.
    pub id: u32,
    /// Mirror horizontally.
    pub flip_x: bool,
}

/// Read-only view of one tile layer.
pub trait TileGrid {
    /// Width and height in tiles.
    fn size(&self) -> (u32, u32);
    /// Tile at `(x, y)`, or `None` when empty.
    fn tile_at(&self, x: u32, y: u32) -> Option<TileInstance>;
}

/// Tile columns and rows that intersect the view whose top-left is `origin` (culling).
#[must_use]
pub fn visible_tiles(
    origin: (i32, i32),
    view: (u32, u32),
    tile: (u32, u32),
    grid: (u32, u32),
) -> (Range<u32>, Range<u32>) {
    (
        visible_axis(origin.0, view.0, tile.0, grid.0),
        visible_axis(origin.1, view.1, tile.1, grid.1),
    )
}

fn visible_axis(origin: i32, view: u32, tile: u32, count: u32) -> Range<u32> {
    if tile == 0 || count == 0 {
        return 0..0;
    }
    let tile = i64::from(tile);
    let start = i64::from(origin).div_euclid(tile).max(0);
    let end = (i64::from(origin) + i64::from(view) + tile - 1)
        .div_euclid(tile)
        .min(i64::from(count));
    if end <= start {
        return 0..0;
    }
    // Both bounds are within 0..=count, which is a u32.
    (start as u32)..(end as u32)
}

/// Builds the draws for the visible part of `grid` on `layer`, in row-major order.
pub fn tile_layer_draws(
    grid: &dyn TileGrid,
    sheets: &[TileSheet],
    tile: (u32, u32),
    layer: Layer,
    origin: (i32, i32),
    view: (u32, u32),
) -> Vec<SpriteDraw> {
    tile_layer_draws_by_sheet(grid, sheets, tile, layer, origin, view)
        .into_iter()
        .map(|(_, draw)| draw)
        .collect()
}

/// [`tile_layer_draws`] with the sheet index of every draw (for per-tileset materials).
pub(crate) fn tile_layer_draws_by_sheet(
    grid: &dyn TileGrid,
    sheets: &[TileSheet],
    tile: (u32, u32),
    layer: Layer,
    origin: (i32, i32),
    view: (u32, u32),
) -> Vec<(u16, SpriteDraw)> {
    let (cols, rows) = visible_tiles(origin, view, tile, grid.size());
    let mut draws = Vec::with_capacity(cols.len() * rows.len());
    for y in rows {
        for x in cols.clone() {
            let Some(t) = grid.tile_at(x, y) else {
                continue;
            };
            let Some(sheet) = sheets.get(usize::from(t.sheet)) else {
                continue;
            };
            let Some(src) = sheet.src(t.id) else { continue };
            let dst_x = i64::from(x) * i64::from(tile.0) - i64::from(origin.0);
            let dst_y = i64::from(y) * i64::from(tile.1) - i64::from(origin.1);
            draws.push((
                t.sheet,
                SpriteDraw {
                    texture: sheet.texture,
                    src,
                    dst: Rect::new(dst_x as f32, dst_y as f32, src.width, src.height),
                    flip_x: t.flip_x,
                    tint: Color::WHITE,
                    layer,
                },
            ));
        }
    }
    draws
}

/// One sprite in the `Entities` layer with its y-sort key (§5.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntitySprite {
    /// Stable id: the tiebreak that prevents flicker for entities on the same row.
    pub id: u32,
    /// Feet Y in world pixels (tile row × tile height + sub-tile offset).
    pub feet_y: f32,
    /// The draw (its `layer` is forced to [`Layer::Entities`]).
    pub draw: SpriteDraw,
}

/// Orders entities back to front: by feet Y (tile row, then sub-pixel offset), then by id.
pub fn y_sort(entities: &mut [EntitySprite]) {
    entities.sort_by(y_order);
}

/// The [`y_sort`] order of two entities.
pub(crate) fn y_order(a: &EntitySprite, b: &EntitySprite) -> Ordering {
    match a.feet_y.total_cmp(&b.feet_y) {
        Ordering::Equal => a.id.cmp(&b.id),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Checker {
        size: (u32, u32),
    }

    impl TileGrid for Checker {
        fn size(&self) -> (u32, u32) {
            self.size
        }
        fn tile_at(&self, x: u32, y: u32) -> Option<TileInstance> {
            (x + y).is_multiple_of(2).then_some(TileInstance {
                sheet: 0,
                id: (x + y) % 4,
                flip_x: false,
            })
        }
    }

    const SHEET: TileSheet = TileSheet {
        texture: TextureHandle(3),
        tile_width: 16,
        tile_height: 16,
        columns: 2,
        tile_count: 4,
        margin: 0,
        spacing: 0,
    };

    #[test]
    fn visible_range_culls_to_view() {
        // 240×160 view at (0,0) over 16px tiles: 15×10 tiles.
        assert_eq!(
            visible_tiles((0, 0), (240, 160), (16, 16), (100, 100)),
            (0..15, 0..10)
        );
        // Partially visible tiles at both edges are included.
        assert_eq!(
            visible_tiles((8, 8), (240, 160), (16, 16), (100, 100)),
            (0..16, 0..11)
        );
        // Negative origin (small, centred map) starts at 0 and stops at the map edge.
        assert_eq!(
            visible_tiles((-40, -32), (240, 160), (16, 16), (10, 6)),
            (0..10, 0..6)
        );
        // Entirely off-screen.
        assert_eq!(
            visible_tiles((5000, 0), (240, 160), (16, 16), (10, 10)),
            (0..0, 0..10)
        );
    }

    #[test]
    fn layer_draws_only_visible_non_empty_tiles() {
        let grid = Checker { size: (40, 40) };
        let draws = tile_layer_draws(&grid, &[SHEET], (16, 16), Layer::Ground, (16, 0), (32, 16));
        // Columns 1..3, row 0 → only (2,0) is non-empty.
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].dst, Rect::new(16.0, 0.0, 16.0, 16.0));
        assert_eq!(draws[0].src, Rect::new(0.0, 16.0, 16.0, 16.0), "tile id 2");
        assert_eq!(draws[0].layer, Layer::Ground);
    }

    #[test]
    fn unknown_sheet_or_tile_id_is_skipped() {
        struct Bad;
        impl TileGrid for Bad {
            fn size(&self) -> (u32, u32) {
                (2, 1)
            }
            fn tile_at(&self, x: u32, _: u32) -> Option<TileInstance> {
                Some(TileInstance {
                    sheet: x as u16,
                    id: 99,
                    flip_x: false,
                })
            }
        }
        assert!(
            tile_layer_draws(&Bad, &[SHEET], (16, 16), Layer::Ground, (0, 0), (32, 16)).is_empty()
        );
    }

    fn entity(id: u32, feet_y: f32) -> EntitySprite {
        EntitySprite {
            id,
            feet_y,
            draw: SpriteDraw {
                texture: TextureHandle(1),
                src: Rect::new(0.0, 0.0, 16.0, 16.0),
                dst: Rect::new(0.0, 0.0, 16.0, 16.0),
                flip_x: false,
                tint: Color::WHITE,
                layer: Layer::Entities,
            },
        }
    }

    #[test]
    fn y_sort_by_feet_then_stable_id() {
        let mut list = vec![
            entity(5, 48.0),
            entity(2, 32.0),
            entity(9, 32.0),
            entity(1, 40.5),
            entity(3, 32.0),
        ];
        y_sort(&mut list);
        let ids: Vec<_> = list.iter().map(|e| e.id).collect();
        assert_eq!(ids, [2, 3, 9, 1, 5]);
    }

    #[test]
    fn y_sort_is_independent_of_input_order() {
        // Same-row entities never swap between frames, whatever order the caller submits.
        let a = [entity(7, 64.0), entity(4, 64.0), entity(6, 64.0)];
        let mut b = a;
        b.reverse();
        let mut a = a;
        y_sort(&mut a);
        y_sort(&mut b);
        assert_eq!(a.map(|e| e.id), b.map(|e| e.id));
    }
}
