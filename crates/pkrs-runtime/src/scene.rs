//! Overworld map scene: draws a [`pkrs_world::Map`] through the [`Renderer`] (§5.2, §5.3).
//!
//! This is the bridge between the world model and the renderer (which cannot see each other,
//! §4.3): world tile layers are exposed as [`TileGrid`]s, map objects and the player become
//! y-sorted [`EntitySprite`]s, and a [`CameraController`] follows the interpolated player.
//!
//! Draw order: `ground` → `ground2…` → entities (y-sorted) → `overhead` (§5.2).

use pkrs_core::GridPos;
use pkrs_render::{
    CameraBounds, CameraController, CameraTarget, Color, EntitySprite, Layer, MaterialHandle, Rect,
    Renderer, SpriteDraw, TextureHandle, TileGrid, TileInstance, TileMaterials, TileSheet, snap,
};

use crate::materials::MapMaterials;
use pkrs_world::{Map, MapTileset, TileLayer, VisualLayerKind};

use crate::fixed_step::InterpolatedPosition;

/// Stable entity id reserved for the player (Tiled object ids start at 1).
pub const PLAYER_ENTITY: u32 = 0;

/// Adapts a world [`TileLayer`] to the renderer's [`TileGrid`].
pub struct WorldTileGrid<'a>(pub &'a TileLayer);

impl TileGrid for WorldTileGrid<'_> {
    fn size(&self) -> (u32, u32) {
        (self.0.width(), self.0.height())
    }

    fn tile_at(&self, x: u32, y: u32) -> Option<TileInstance> {
        self.0.tile(x, y).map(|t| TileInstance {
            sheet: t.tileset,
            id: t.id,
            flip_x: t.flip_h,
        })
    }
}

/// Render band for a world visual layer kind.
#[must_use]
pub const fn layer_for(kind: VisualLayerKind) -> Layer {
    match kind {
        VisualLayerKind::Ground => Layer::Ground,
        VisualLayerKind::GroundDecor => Layer::GroundDecor,
        VisualLayerKind::Overhead => Layer::Overhead,
    }
}

/// Builds the renderer's [`TileSheet`] for a world tileset uploaded as `texture`.
#[must_use]
pub fn tile_sheet(tileset: &MapTileset, texture: TextureHandle) -> TileSheet {
    TileSheet {
        texture,
        tile_width: tileset.tile_width,
        tile_height: tileset.tile_height,
        columns: tileset.columns,
        tile_count: tileset.tile_count,
        margin: tileset.margin,
        spacing: tileset.spacing,
    }
}

/// A sprite on the map (player or NPC).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneEntity {
    /// Stable id (y-sort tiebreak; [`PLAYER_ENTITY`] for the player, object id for NPCs).
    pub id: u32,
    /// Top-left of the occupied tile in world pixels, interpolated between ticks.
    pub pos: InterpolatedPosition,
    /// Sprite sheet.
    pub texture: TextureHandle,
    /// Current frame in the sheet; the sprite's bottom edge sits on the tile's bottom edge.
    pub src: Rect,
    /// Mirror horizontally.
    pub flip_x: bool,
    /// Material the sprite is drawn with (§5.6), e.g. from an NPC's `material` property.
    pub material: Option<MaterialHandle>,
}

/// Error building a [`MapScene`].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SceneError {
    /// One texture per map tileset is required.
    #[error("map `{map}` has {expected} tileset(s) but {found} texture(s) were given")]
    TilesetCount {
        /// Map id.
        map: String,
        /// Tilesets on the map.
        expected: usize,
        /// Textures given.
        found: usize,
    },
}

/// A neighbouring map drawn around the active map (§6.4 edge strips).
pub struct NeighborBackdrop {
    /// Neighbour map.
    pub map: Map,
    /// Uploaded tilesets matching [`Map::tilesets`].
    pub sheets: Vec<TileSheet>,
    /// Pixel offset from the active map's origin.
    pub offset_px: (f32, f32),
}

/// A loaded map plus its entities and camera.
pub struct MapScene {
    map: Map,
    sheets: Vec<TileSheet>,
    entities: Vec<SceneEntity>,
    camera: CameraController,
    materials: MapMaterials,
}

impl MapScene {
    /// Wraps `map`; `textures[i]` is the uploaded image of `map.tilesets[i]`.
    pub fn new(map: Map, textures: &[TextureHandle]) -> Result<Self, SceneError> {
        if textures.len() != map.tilesets.len() {
            return Err(SceneError::TilesetCount {
                map: map.id.clone(),
                expected: map.tilesets.len(),
                found: textures.len(),
            });
        }
        let sheets = map
            .tilesets
            .iter()
            .zip(textures)
            .map(|(ts, &tex)| tile_sheet(ts, tex))
            .collect();
        Ok(Self {
            map,
            sheets,
            entities: Vec::new(),
            camera: CameraController::new(),
            materials: MapMaterials::default(),
        })
    }

    /// Sets the tileset and layer materials (§5.6), indexed like the map's tilesets and
    /// layers.
    pub fn set_materials(&mut self, materials: MapMaterials) {
        self.materials = materials;
    }

    /// The tileset and layer materials.
    #[must_use]
    pub fn materials(&self) -> &MapMaterials {
        &self.materials
    }

    /// Draws entity `id` with `material` (`None`: plain); returns whether it exists.
    pub fn set_entity_material(&mut self, id: u32, material: Option<MaterialHandle>) -> bool {
        match self.entities.iter_mut().find(|e| e.id == id) {
            Some(entity) => {
                entity.material = material;
                true
            }
            None => false,
        }
    }

    /// The map.
    #[must_use]
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// Takes the map and its uploaded tile textures (for neighbour cache reuse, §6.4).
    pub fn detach(self) -> (Map, Vec<TextureHandle>) {
        let textures = self.sheets.iter().map(|s| s.texture).collect();
        (self.map, textures)
    }

    /// Camera behaviour (`pan_to`, `follow`, `shake`).
    pub fn camera_mut(&mut self) -> &mut CameraController {
        &mut self.camera
    }

    /// World-pixel top-left of tile `pos`.
    #[must_use]
    pub fn tile_to_px(&self, pos: GridPos) -> (f32, f32) {
        (
            pos.x as f32 * self.map.tile_width as f32,
            pos.y as f32 * self.map.tile_height as f32,
        )
    }

    /// Adds (or replaces, by id) an entity standing on `tile`.
    pub fn place_entity(
        &mut self,
        id: u32,
        tile: GridPos,
        texture: TextureHandle,
        src: Rect,
        flip_x: bool,
    ) {
        let (x, y) = self.tile_to_px(tile);
        let mut pos = InterpolatedPosition::default();
        pos.teleport(x, y);
        let entity = SceneEntity {
            id,
            pos,
            texture,
            src,
            flip_x,
            material: None,
        };
        match self.entities.iter_mut().find(|e| e.id == id) {
            Some(existing) => *existing = entity,
            None => self.entities.push(entity),
        }
    }

    /// Removes the entity `id` (e.g. a hidden NPC); returns whether it existed.
    pub fn remove_entity(&mut self, id: u32) -> bool {
        let before = self.entities.len();
        self.entities.retain(|e| e.id != id);
        self.entities.len() != before
    }

    /// Entity by id.
    pub fn entity_mut(&mut self, id: u32) -> Option<&mut SceneEntity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    /// All entities in insertion order.
    #[must_use]
    pub fn entities(&self) -> &[SceneEntity] {
        &self.entities
    }

    /// One simulation tick of camera behaviour.
    pub fn tick(&mut self) {
        self.camera.tick();
    }

    fn entity_centre(&self, id: u32, alpha: f32) -> Option<(f32, f32)> {
        let e = self.entities.iter().find(|e| e.id == id)?;
        let (x, y) = e.pos.at(alpha);
        Some((
            x + self.map.tile_width as f32 / 2.0,
            y + self.map.tile_height as f32 / 2.0,
        ))
    }

    /// Queues the whole map into `renderer` for this frame (between `begin_frame` and
    /// `end_frame`). Returns the camera origin used.
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        alpha: f32,
        neighbors: &[&NeighborBackdrop],
    ) -> (i32, i32) {
        let view = renderer.view_size();
        let target = match self.camera.target() {
            CameraTarget::Player => self.entity_centre(PLAYER_ENTITY, alpha),
            CameraTarget::Entity(id) => self.entity_centre(id, alpha),
            CameraTarget::Fixed { .. } => None,
        };
        let bounds = extended_camera_bounds(self.map(), neighbors);
        let camera = self.camera.resolve(target, alpha, view, bounds);
        let origin = camera.origin(view);
        let tile = (self.map.tile_width, self.map.tile_height);

        let draw_map = |renderer: &mut Renderer,
                        map: &Map,
                        sheets: &[TileSheet],
                        offset: (f32, f32),
                        origin: (i32, i32),
                        kinds: &[VisualLayerKind]| {
            let tile = (map.tile_width, map.tile_height);
            let layer_origin = (
                origin.0 + offset.0.round() as i32,
                origin.1 + offset.1.round() as i32,
            );
            for layer in map
                .layers
                .iter()
                .filter(|l| l.visible && kinds.contains(&l.kind))
            {
                renderer.draw_tile_layer(
                    &WorldTileGrid(layer),
                    sheets,
                    tile,
                    layer_for(layer.kind),
                    layer_origin,
                );
            }
        };

        for neighbor in neighbors {
            draw_map(
                renderer,
                &neighbor.map,
                &neighbor.sheets,
                neighbor.offset_px,
                origin,
                &[VisualLayerKind::Ground, VisualLayerKind::GroundDecor],
            );
        }

        for (index, layer) in self.map.layers.iter().enumerate() {
            if !layer.visible {
                continue;
            }
            renderer.draw_tile_layer_with(
                &WorldTileGrid(layer),
                &self.sheets,
                tile,
                layer_for(layer.kind),
                origin,
                TileMaterials {
                    layer: self.materials.layers.get(index).copied().flatten(),
                    sheets: &self.materials.tilesets,
                },
            );
        }

        renderer.set_map_origin((-(origin.0 as f32), -(origin.1 as f32)));
        let tile_h = self.map.tile_height as f32;
        let entity_materials: Vec<Option<MaterialHandle>> =
            self.entities.iter().map(|e| e.material).collect();
        let sprites: Vec<EntitySprite> = self
            .entities
            .iter()
            .map(|e| {
                let (x, y) = e.pos.at(alpha);
                let feet_y = y + tile_h;
                let dst_x = snap(x) - origin.0;
                let dst_y = snap(feet_y) - snap(e.src.height) - origin.1;
                EntitySprite {
                    id: e.id,
                    feet_y,
                    draw: SpriteDraw {
                        texture: e.texture,
                        src: e.src,
                        dst: Rect::new(dst_x as f32, dst_y as f32, e.src.width, e.src.height),
                        flip_x: e.flip_x,
                        tint: Color::WHITE,
                        layer: Layer::Entities,
                    },
                }
            })
            .collect();
        renderer.draw_entities_with(&sprites, &entity_materials);

        for neighbor in neighbors {
            draw_map(
                renderer,
                &neighbor.map,
                &neighbor.sheets,
                neighbor.offset_px,
                origin,
                &[VisualLayerKind::Overhead],
            );
        }
        origin
    }
}

#[must_use]
fn extended_camera_bounds(map: &Map, neighbors: &[&NeighborBackdrop]) -> CameraBounds {
    let (tw, th) = (map.tile_width, map.tile_height);
    let (mut min_x, mut min_y) = (0f32, 0f32);
    let (mut max_x, mut max_y) = (map.width as f32 * tw as f32, map.height as f32 * th as f32);
    for n in neighbors {
        let (nx, ny) = n.offset_px;
        let (nw, nh) = n.map.pixel_size();
        min_x = min_x.min(nx);
        min_y = min_y.min(ny);
        max_x = max_x.max(nx + nw as f32);
        max_y = max_y.max(ny + nh as f32);
    }
    CameraBounds {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_platform::DisplaySettings;
    use pkrs_render::{DrawLog, NullRenderer, RendererConfig};
    use pkrs_world::{EncounterZone, LogicalGrid, Properties, TileRef, VisualLayerKind as V};

    fn headless_renderer(display: DisplaySettings) -> (Renderer, DrawLog) {
        let null = NullRenderer::new();
        let log = null.draw_log();
        (
            Renderer::new(Box::new(null), display, RendererConfig::default()),
            log,
        )
    }

    fn map(w: u32, h: u32) -> Map {
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
                TileLayer::new("ground", V::Ground, w, h, full(0)).unwrap(),
                TileLayer::new("overhead", V::Overhead, w, h, full(1)).unwrap(),
            ],
            grid: LogicalGrid::new(w, h),
            objects: vec![],
            encounters: Vec::<EncounterZone>::new(),
        }
    }

    #[test]
    fn rejects_texture_count_mismatch() {
        assert!(matches!(
            MapScene::new(map(2, 2), &[]),
            Err(SceneError::TilesetCount {
                expected: 1,
                found: 0,
                ..
            })
        ));
    }

    #[test]
    fn walking_player_keeps_screen_position_and_tiles_cull() {
        let (mut renderer, log) = headless_renderer(DisplaySettings::default());
        let mut scene = MapScene::new(map(40, 40), &[TextureHandle(1)]).unwrap();
        let sprite = Rect::new(0.0, 0.0, 16.0, 16.0);
        scene.place_entity(
            PLAYER_ENTITY,
            GridPos::new(20, 20),
            TextureHandle(2),
            sprite,
            false,
        );
        // One step right, interpolated over the tick.
        let (x, y) = scene.tile_to_px(GridPos::new(21, 20));
        scene.entity_mut(PLAYER_ENTITY).unwrap().pos.set(x, y);

        let mut player_screen = Vec::new();
        for i in 0..=8 {
            renderer.begin_frame(240, 160);
            scene.render(&mut renderer, i as f32 / 8.0, &[]);
            renderer.end_frame();
            let draws = log.draws();
            let player = draws.iter().find(|d| d.layer == Layer::Entities).unwrap();
            player_screen.push((player.dst.x, player.dst.y));
            // 240×160 view over 16px tiles: at most 16×11 tiles per layer when unaligned.
            let ground = draws.iter().filter(|d| d.layer == Layer::Ground).count();
            assert!(ground <= 16 * 11, "culled: {ground}");
            drop(draws);
            log.clear();
        }
        assert!(
            player_screen.iter().all(|&p| p == player_screen[0]),
            "no sub-pixel shimmer: {player_screen:?}"
        );
    }
}
