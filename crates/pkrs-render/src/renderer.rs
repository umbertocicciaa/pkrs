//! The `Renderer` facade (§4.5, §5.1, §5.6).

use std::collections::HashMap;

use pkrs_platform::DisplaySettings;

use crate::backend::RenderBackend;
use crate::color::Color;
use crate::draw::SpriteDraw;
use crate::layer::Layer;
use crate::material::{
    EngineUniforms, GlobalUniforms, Material, MaterialBind, MaterialHandle, UniformValue,
};
use crate::rect::Rect;
use crate::texture::TextureHandle;
use crate::tilemap::{
    EntitySprite, TileGrid, TileSheet, tile_layer_draws_by_sheet, y_order, y_sort,
};
use crate::viewport::{ViewportLayout, ViewportMode, compute_viewport};

/// Engine policy applied on top of [`RenderBackend`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RendererConfig {
    /// Letterbox / pillarbox bars around the integer-scaled image.
    pub letterbox: Color,
    /// Clear colour on the internal render target each frame.
    pub clear: Color,
    /// Integer vs fit scaling (§5.1).
    pub viewport_mode: ViewportMode,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            letterbox: Color::BLACK,
            clear: Color::BLACK,
            viewport_mode: ViewportMode::Integer,
        }
    }
}

/// Materials for one tile layer (§5.6): the layer's own (Tiled layer property `material`)
/// and one per sheet (tileset property `material`), indexed like the `sheets` slice. A
/// tile uses its sheet's material when that material applies to the tile (its
/// `atlas_region`), else the layer's, else none.
#[derive(Clone, Copy, Debug, Default)]
pub struct TileMaterials<'a> {
    /// Material of the whole layer.
    pub layer: Option<MaterialHandle>,
    /// Material per sheet.
    pub sheets: &'a [Option<MaterialHandle>],
}

/// Game-facing renderer: internal target, viewport, layer ordering, materials.
pub struct Renderer {
    backend: Box<dyn RenderBackend>,
    display: DisplaySettings,
    config: RendererConfig,
    pending: Vec<Queued>,
    /// Material binds of this frame, referenced by index from `pending`.
    binds: Vec<MaterialBind>,
    materials: HashMap<MaterialHandle, Material>,
    globals: GlobalUniforms,
    time_ticks: u64,
    map_origin: (f32, f32),
    texture_sizes: HashMap<TextureHandle, (u32, u32)>,
}

enum Queued {
    Sprite(SpriteDraw, Option<usize>),
    Rect {
        dst: Rect,
        color: Color,
        layer: Layer,
    },
}

impl Queued {
    fn layer(&self) -> Layer {
        match self {
            Self::Sprite(draw, _) => draw.layer,
            Self::Rect { layer, .. } => *layer,
        }
    }
}

impl Renderer {
    /// Wraps `backend` with project display settings and policy.
    pub fn new(
        backend: Box<dyn RenderBackend>,
        display: DisplaySettings,
        config: RendererConfig,
    ) -> Self {
        Self {
            backend,
            display,
            config,
            pending: Vec::new(),
            binds: Vec::new(),
            materials: HashMap::new(),
            globals: GlobalUniforms::default(),
            time_ticks: 0,
            map_origin: (0.0, 0.0),
            texture_sizes: HashMap::new(),
        }
    }

    /// Internal resolution from `pkrs.toml [display]`.
    pub fn display(&self) -> &DisplaySettings {
        &self.display
    }

    /// Internal view size `(width, height)` in pixels (the camera's view).
    pub fn view_size(&self) -> (u32, u32) {
        (self.display.width, self.display.height)
    }

    /// Facade configuration (letterbox, scale mode, …).
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// Computes viewport layout for the current window size.
    pub fn viewport_for_window(&self, window_width: u32, window_height: u32) -> ViewportLayout {
        compute_viewport(
            self.display.width,
            self.display.height,
            window_width,
            window_height,
            self.config.viewport_mode,
        )
    }

    /// Starts a frame: resize internal target, letterbox, clear.
    pub fn begin_frame(&mut self, window_width: u32, window_height: u32) {
        self.backend
            .resize_internal(self.display.width, self.display.height);
        let viewport = self.viewport_for_window(window_width, window_height);
        self.backend.begin_frame(&viewport, self.config.letterbox);
        self.backend.clear_internal(self.config.clear);
        self.pending.clear();
        self.binds.clear();
    }

    /// Queues a sprite draw (flushed in layer order in [`end_frame`](Self::end_frame)).
    pub fn draw_sprite(&mut self, draw: SpriteDraw) {
        self.pending.push(Queued::Sprite(draw, None));
    }

    /// Queues a sprite drawn with `material` (when it applies to `draw.src`), its uniforms
    /// resolved now: `time_ticks`, the current [`map_origin`](Self::set_map_origin), the
    /// texture size and the global uniforms.
    pub fn draw_sprite_with_material(
        &mut self,
        draw: SpriteDraw,
        material: Option<MaterialHandle>,
    ) {
        let bind =
            material.and_then(|m| self.bind_for(m, draw, self.map_origin, &mut HashMap::new()));
        self.pending.push(Queued::Sprite(draw, bind));
    }

    /// Queues a solid rectangle on `layer` (e.g. UI window chrome), ordered with the
    /// sprites like any other draw.
    pub fn draw_filled_rect(&mut self, dst: Rect, color: Color, layer: Layer) {
        self.pending.push(Queued::Rect { dst, color, layer });
    }

    /// Queues the visible tiles of `grid` on `layer`; `origin` is the camera's
    /// [`origin`](crate::Camera::origin). Returns how many tiles were queued.
    pub fn draw_tile_layer(
        &mut self,
        grid: &dyn TileGrid,
        sheets: &[TileSheet],
        tile_size: (u32, u32),
        layer: Layer,
        origin: (i32, i32),
    ) -> usize {
        self.draw_tile_layer_with(
            grid,
            sheets,
            tile_size,
            layer,
            origin,
            TileMaterials::default(),
        )
    }

    /// Like [`draw_tile_layer`](Self::draw_tile_layer), with materials (§5.6). The layer's
    /// `map_origin` uniform is `-origin`, the map's top-left on the internal target.
    pub fn draw_tile_layer_with(
        &mut self,
        grid: &dyn TileGrid,
        sheets: &[TileSheet],
        tile_size: (u32, u32),
        layer: Layer,
        origin: (i32, i32),
        materials: TileMaterials<'_>,
    ) -> usize {
        let view = self.view_size();
        let map_origin = (-(origin.0 as f32), -(origin.1 as f32));
        let mut cache = HashMap::new();
        let mut count = 0;
        for (sheet, draw) in tile_layer_draws_by_sheet(grid, sheets, tile_size, layer, origin, view)
        {
            let sheet = materials.sheets.get(usize::from(sheet)).copied().flatten();
            let bind = [sheet, materials.layer]
                .into_iter()
                .flatten()
                .find_map(|m| self.bind_for(m, draw, map_origin, &mut cache));
            self.pending.push(Queued::Sprite(draw, bind));
            count += 1;
        }
        count
    }

    /// Y-sorts `entities` (feet Y, then stable id) and queues them on [`Layer::Entities`].
    pub fn draw_entities(&mut self, entities: &mut [EntitySprite]) {
        y_sort(entities);
        self.pending.extend(entities.iter().map(|e| {
            Queued::Sprite(
                SpriteDraw {
                    layer: Layer::Entities,
                    ..e.draw
                },
                None,
            )
        }));
    }

    /// Like [`draw_entities`](Self::draw_entities), with a material per entity (§5.6);
    /// `materials[i]` belongs to `entities[i]` before sorting.
    pub fn draw_entities_with(
        &mut self,
        entities: &[EntitySprite],
        materials: &[Option<MaterialHandle>],
    ) {
        let mut order: Vec<(EntitySprite, Option<MaterialHandle>)> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| (*e, materials.get(i).copied().flatten()))
            .collect();
        order.sort_by(|(a, _), (b, _)| y_order(a, b));
        let mut cache = HashMap::new();
        for (e, material) in order {
            let draw = SpriteDraw {
                layer: Layer::Entities,
                ..e.draw
            };
            let bind = material.and_then(|m| self.bind_for(m, draw, self.map_origin, &mut cache));
            self.pending.push(Queued::Sprite(draw, bind));
        }
    }

    /// Submits queued draws and presents to the window.
    ///
    /// The sort by layer is stable, so draws within one layer keep their submission order
    /// (file order for tile layers, y-sort order for entities).
    pub fn end_frame(&mut self) {
        self.pending.sort_by_key(Queued::layer);
        for queued in &self.pending {
            match queued {
                Queued::Sprite(draw, None) => self.backend.draw_sprite(draw),
                Queued::Sprite(draw, Some(bind)) => match self.binds.get(*bind) {
                    Some(bind) => self.backend.draw_sprite_with_material(draw, bind),
                    None => self.backend.draw_sprite(draw),
                },
                Queued::Rect { dst, color, layer } => {
                    self.backend.draw_filled_rect(*dst, *color, *layer);
                }
            }
        }
        self.pending.clear();
        self.binds.clear();
        self.backend.end_frame();
    }

    /// Decodes and uploads an encoded image (PNG); returns the handle and its size.
    pub fn load_texture(&mut self, encoded: &[u8]) -> crate::Result<(TextureHandle, u32, u32)> {
        let (handle, w, h) = self.backend.create_texture_from_image(encoded)?;
        self.texture_sizes.insert(handle, (w, h));
        Ok((handle, w, h))
    }

    /// Uploads an RGBA8 sprite sheet through the backend.
    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> crate::Result<TextureHandle> {
        let handle = self.backend.create_texture(width, height, rgba)?;
        self.texture_sizes.insert(handle, (width, height));
        Ok(handle)
    }

    /// Compiles `material` through the backend (§5.6); `name` is its id for logs.
    pub fn create_material(
        &mut self,
        name: &str,
        material: Material,
    ) -> crate::Result<MaterialHandle> {
        let handle = self.backend.create_material(&material.program(name))?;
        self.materials.insert(handle, material);
        Ok(handle)
    }

    /// The material behind `handle`.
    #[must_use]
    pub fn material(&self, handle: MaterialHandle) -> Option<&Material> {
        self.materials.get(&handle)
    }

    /// Sets a global uniform read by `Global(name: …)` material uniforms (plugin globals
    /// such as a day/night tint, §5.6). It applies from the next queued draw on.
    pub fn set_global_uniform(&mut self, name: &str, value: UniformValue) {
        self.globals.set(name, value);
    }

    /// Clears a global uniform (materials fall back to their defaults).
    pub fn clear_global_uniform(&mut self, name: &str) -> bool {
        self.globals.remove(name)
    }

    /// The global uniforms.
    #[must_use]
    pub fn global_uniforms(&self) -> &GlobalUniforms {
        &self.globals
    }

    /// Sets the `time_ticks` engine uniform (the simulation tick count, §3.1).
    pub fn set_time_ticks(&mut self, ticks: u64) {
        self.time_ticks = ticks;
    }

    /// The `time_ticks` engine uniform.
    #[must_use]
    pub fn time_ticks(&self) -> u64 {
        self.time_ticks
    }

    /// Sets the `map_origin` engine uniform for sprites (the map's top-left on the internal
    /// target, in pixels). Tile layers use their own origin.
    pub fn set_map_origin(&mut self, origin: (f32, f32)) {
        self.map_origin = origin;
    }

    /// Resolves `material` for `draw` and stores the bind; `None` when the material is
    /// unknown or does not apply to `draw.src`. `cache` reuses binds per (material, texture).
    fn bind_for(
        &mut self,
        material: MaterialHandle,
        draw: SpriteDraw,
        map_origin: (f32, f32),
        cache: &mut HashMap<(MaterialHandle, TextureHandle), usize>,
    ) -> Option<usize> {
        let m = self.materials.get(&material)?;
        if !m.applies_to(draw.src) {
            return None;
        }
        if let Some(index) = cache.get(&(material, draw.texture)) {
            return Some(*index);
        }
        let engine = EngineUniforms {
            time_ticks: self.time_ticks,
            map_origin,
            texture_size: self
                .texture_sizes
                .get(&draw.texture)
                .copied()
                .unwrap_or((0, 0)),
        };
        let uniforms = m.resolve_uniforms(&engine, &self.globals);
        self.binds.push(MaterialBind { material, uniforms });
        let index = self.binds.len() - 1;
        cache.insert((material, draw.texture), index);
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::NullRenderer;
    use crate::layer::Layer;
    use crate::rect::Rect;
    use crate::texture::TextureHandle;

    #[test]
    fn flushes_draws_sorted_by_layer() {
        let null = NullRenderer::new();
        let log = null.draw_log();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        renderer.begin_frame(720, 480);
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            tint: Color::WHITE,
            layer: Layer::Ui,
        });
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            tint: Color::WHITE,
            layer: Layer::Ground,
        });
        renderer.end_frame();
        let layers: Vec<_> = log.draws().iter().map(|d| d.layer).collect();
        assert_eq!(layers, vec![Layer::Ground, Layer::Ui]);
    }

    #[test]
    fn filled_rects_keep_submission_order_with_sprites() {
        let null = NullRenderer::new();
        let log = null.draw_log();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        renderer.begin_frame(720, 480);
        let panel = SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 64.0, 16.0),
            flip_x: false,
            tint: Color::WHITE,
            layer: Layer::Ui,
        };
        renderer.draw_sprite(panel);
        renderer.draw_filled_rect(Rect::new(4.0, 4.0, 32.0, 4.0), Color::BLACK, Layer::Ui);
        renderer.draw_filled_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Color::BLACK, Layer::Ground);
        assert!(
            log.draws().is_empty(),
            "nothing reaches the backend before end_frame"
        );
        renderer.end_frame();
        let order: Vec<_> = log
            .draws()
            .iter()
            .map(|d| (d.layer, d.is_filled_rect))
            .collect();
        assert_eq!(
            order,
            vec![(Layer::Ground, true), (Layer::Ui, false), (Layer::Ui, true),],
            "a bar drawn after its panel stays on top of it"
        );
    }
}
