//! F4 collision, terrain, warp and trigger overlay (§15.1).

use pkrs_core::GridPos;
use pkrs_render::{Color, Layer, Rect, Renderer};
use pkrs_world::{LogicalGrid, Map, ObjectKind};

const SOLID: Color = Color {
    r: 220,
    g: 40,
    b: 40,
    a: 90,
};
const TERRAIN: Color = Color {
    r: 40,
    g: 200,
    b: 80,
    a: 70,
};
const WARP: Color = Color {
    r: 80,
    g: 120,
    b: 255,
    a: 110,
};
const TRIGGER: Color = Color {
    r: 255,
    g: 200,
    b: 40,
    a: 100,
};
const ENCOUNTER: Color = Color {
    r: 180,
    g: 60,
    b: 255,
    a: 80,
};

/// Draws semi-transparent tiles for collision and map objects.
pub fn render_collision_view(
    renderer: &mut Renderer,
    map: &Map,
    grid: &LogicalGrid,
    origin: (i32, i32),
    tile: (u32, u32),
) {
    let (tw, th) = (tile.0 as f32, tile.1 as f32);
    let (ox, oy) = (origin.0 as f32, origin.1 as f32);
    let mut tile_rect = |pos: GridPos, color: Color| {
        let x = pos.x as f32 * tw - ox;
        let y = pos.y as f32 * th - oy;
        renderer.draw_filled_rect(Rect::new(x, y, tw, th), color, Layer::WeatherFx);
    };
    for y in 0..map.height {
        for x in 0..map.width {
            let pos = GridPos::new(x as i32, y as i32);
            let Some(cell) = grid.cell(pos) else { continue };
            if cell.solid {
                tile_rect(pos, SOLID);
            } else if cell.terrain.is_some() {
                tile_rect(pos, TERRAIN);
            }
        }
    }
    for zone in &map.encounters {
        for dy in 0..zone.rect.height {
            for dx in 0..zone.rect.width {
                tile_rect(
                    GridPos::new(zone.rect.x + dx as i32, zone.rect.y + dy as i32),
                    ENCOUNTER,
                );
            }
        }
    }
    for obj in &map.objects {
        let color = match &obj.kind {
            ObjectKind::Warp(_) => WARP,
            ObjectKind::Trigger { .. } => TRIGGER,
            _ => continue,
        };
        for dy in 0..obj.rect.height {
            for dx in 0..obj.rect.width {
                tile_rect(
                    GridPos::new(obj.rect.x + dx as i32, obj.rect.y + dy as i32),
                    color,
                );
            }
        }
    }
}
