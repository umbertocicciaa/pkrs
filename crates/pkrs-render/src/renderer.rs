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
    /// Atlas regions (§5.2): handle → where it sits on its page.
    regions: HashMap<TextureHandle, TextureRegion>,
    next_region: u32,
    /// Textures uploaded once under a name (atlas pages).
    named: HashMap<String, (TextureHandle, u32, u32)>,
}

/// Handles at or above this value are atlas regions made by the facade, never backend
/// textures.
const REGION_HANDLE_BASE: u32 = 0x8000_0000;

/// A rectangle of a page texture, addressed through its own [`TextureHandle`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct TextureRegion {
    page: TextureHandle,
    x: f32,
    y: f32,
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
            regions: HashMap::new(),
            next_region: REGION_HANDLE_BASE,
            named: HashMap::new(),
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
                Queued::Sprite(draw, None) => self.backend.draw_sprite(&self.resolve(draw)),
                Queued::Sprite(draw, Some(bind)) => match self.binds.get(*bind) {
                    Some(bind) => self
                        .backend
                        .draw_sprite_with_material(&self.resolve(draw), bind),
                    None => self.backend.draw_sprite(&self.resolve(draw)),
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

    /// The texture uploaded earlier under `name` with
    /// [`load_named_texture`](Self::load_named_texture), with its size.
    #[must_use]
    pub fn named_texture(&self, name: &str) -> Option<(TextureHandle, u32, u32)> {
        self.named.get(name).copied()
    }

    /// Like [`load_texture`](Self::load_texture), remembered under `name` so it is uploaded
    /// once (atlas pages, §5.2). Loading a name again returns the first texture.
    pub fn load_named_texture(
        &mut self,
        name: &str,
        encoded: &[u8],
    ) -> crate::Result<(TextureHandle, u32, u32)> {
        if let Some(found) = self.named_texture(name) {
            return Ok(found);
        }
        let loaded = self.load_texture(encoded)?;
        self.named.insert(name.to_owned(), loaded);
        Ok(loaded)
    }

    /// A handle for the `width`×`height` rectangle at (`x`, `y`) of `page` (an atlas image,
    /// §5.2). The handle behaves like a texture of that size: draw sources are relative to
    /// the rectangle, and the renderer moves them onto the page when it submits the frame,
    /// so callers cannot tell an atlas region from a texture of its own.
    pub fn create_texture_region(
        &mut self,
        page: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> crate::Result<TextureHandle> {
        let invalid = |detail: String| crate::RenderError::TextureRegion { detail };
        if self.regions.contains_key(&page) {
            return Err(invalid("a region's page cannot itself be a region".into()));
        }
        let (pw, ph) = self
            .texture_sizes
            .get(&page)
            .copied()
            .ok_or_else(|| invalid(format!("unknown page texture {}", page.0)))?;
        let fits = width > 0
            && height > 0
            && x.checked_add(width).is_some_and(|r| r <= pw)
            && y.checked_add(height).is_some_and(|b| b <= ph);
        if !fits {
            return Err(invalid(format!(
                "{width}x{height} at {x},{y} does not fit on a {pw}x{ph} page"
            )));
        }
        let handle = TextureHandle(self.next_region);
        self.next_region = self
            .next_region
            .checked_add(1)
            .ok_or_else(|| invalid("too many texture regions".into()))?;
        self.regions.insert(
            handle,
            TextureRegion {
                page,
                x: x as f32,
                y: y as f32,
            },
        );
        self.texture_sizes.insert(handle, (width, height));
        Ok(handle)
    }

    /// `draw` with an atlas region's texture and source moved onto its page.
    fn resolve(&self, draw: &SpriteDraw) -> SpriteDraw {
        match self.regions.get(&draw.texture) {
            Some(region) => SpriteDraw {
                texture: region.page,
                src: Rect::new(
                    draw.src.x + region.x,
                    draw.src.y + region.y,
                    draw.src.width,
                    draw.src.height,
                ),
                ..*draw
            },
            None => *draw,
        }
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

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png
    }

    #[test]
    fn atlas_regions_draw_from_their_page() {
        let null = NullRenderer::new();
        let log = null.draw_log();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        let (page, _, _) = renderer
            .load_named_texture("atlases/tilesets_0.png", &png_header(64, 32))
            .unwrap();
        let region = renderer.create_texture_region(page, 16, 8, 32, 24).unwrap();
        assert_ne!(region, page);
        renderer.begin_frame(240, 160);
        let draw = SpriteDraw {
            texture: region,
            src: Rect::new(8.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: true,
            tint: Color::WHITE,
            layer: Layer::Ground,
        };
        renderer.draw_sprite(draw);
        renderer.draw_sprite(SpriteDraw {
            texture: page,
            ..draw
        });
        renderer.end_frame();
        let draws = log.draws();
        assert_eq!(draws[0].texture, page);
        assert_eq!(draws[0].src, Rect::new(24.0, 8.0, 8.0, 8.0));
        assert_eq!(draws[0].dst, draw.dst);
        assert!(draws[0].flip_x);
        assert_eq!(
            draws[1].src,
            Rect::new(8.0, 0.0, 8.0, 8.0),
            "page draws are untouched"
        );
    }

    #[test]
    fn named_textures_upload_once() {
        let mut renderer = Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        assert!(renderer.named_texture("page").is_none());
        let first = renderer
            .load_named_texture("page", &png_header(16, 16))
            .unwrap();
        let again = renderer
            .load_named_texture("page", &png_header(99, 99))
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(renderer.named_texture("page"), Some(first));
    }

    #[test]
    fn texture_regions_must_fit_their_page() {
        let mut renderer = Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        let (page, _, _) = renderer.load_texture(&png_header(32, 32)).unwrap();
        assert!(renderer.create_texture_region(page, 16, 16, 16, 16).is_ok());
        let err = renderer
            .create_texture_region(page, 20, 0, 16, 16)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid texture region: 16x16 at 20,0 does not fit on a 32x32 page"
        );
        assert!(renderer.create_texture_region(page, 0, 0, 0, 4).is_err());
        assert!(
            renderer
                .create_texture_region(TextureHandle(777), 0, 0, 1, 1)
                .is_err()
        );
        let region = renderer.create_texture_region(page, 0, 0, 8, 8).unwrap();
        assert!(renderer.create_texture_region(region, 0, 0, 1, 1).is_err());
    }
}
