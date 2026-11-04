//! M1 demo: one tick-driven animated sprite on the internal render target (§5.4).

use pkrs_platform::Platform;
use pkrs_render::{
    ClipPlayer, Layer, NullRenderer, Rect, RenderBackend, Renderer, RendererConfig, SpriteDraw,
    SpriteSheetAnimation, TextureHandle, clip_for_direction, parse_animation_descriptor,
};

use crate::SimulationHost;

const DEMO_RON: &str = include_str!("../demo-assets/mascot.ron");
const DEMO_RGBA: &[u8] = include_bytes!("../demo-assets/mascot.rgba");
const DEMO_WIDTH: u32 = 64;
const DEMO_HEIGHT: u32 = 32;

/// Host that plays the bundled mascot walk cycle and draws it each frame.
pub struct AnimationDemoHost {
    renderer: Renderer,
    player: ClipPlayer,
    sheet: SpriteSheetAnimation,
    texture: TextureHandle,
}

impl AnimationDemoHost {
    /// Headless / CI demo using [`NullRenderer`].
    pub fn headless() -> pkrs_render::Result<(Self, pkrs_render::DrawLog)> {
        let null = NullRenderer::new();
        let log = null.draw_log();
        let host = Self::new(null)?;
        Ok((host, log))
    }

    /// Builds the demo with the given render backend (already on the main thread for macroquad).
    pub fn new(backend: impl RenderBackend + 'static) -> pkrs_render::Result<Self> {
        let sheet = parse_animation_descriptor(DEMO_RON)?;
        let resolved = clip_for_direction(&sheet, "walk", "down")?;
        let mut renderer = Renderer::new(
            Box::new(backend),
            pkrs_platform::DisplaySettings::default(),
            RendererConfig::default(),
        );
        let texture = renderer.create_texture(DEMO_WIDTH, DEMO_HEIGHT, DEMO_RGBA)?;
        Ok(Self {
            player: ClipPlayer::new(resolved.clip),
            sheet,
            texture,
            renderer,
        })
    }

    fn draw_sprite(&mut self, platform: &Platform) {
        let (window_w, window_h) = platform.window_size();
        self.renderer.begin_frame(window_w, window_h);
        let frame = self.player.current_frame_index();
        let src = self.sheet.frame_src_rect(frame);
        self.renderer.draw_sprite(SpriteDraw {
            texture: self.texture,
            src,
            dst: Rect::new(112.0, 72.0, 16.0, 16.0),
            flip_x: false,
            layer: Layer::Entities,
        });
        self.renderer.end_frame();
    }
}

impl SimulationHost for AnimationDemoHost {
    fn poll_input(&mut self, _platform: &Platform) {}

    fn begin_tick(&mut self) {}

    fn simulation_tick(&mut self, _dt: f64) {
        self.player.tick();
    }

    fn render(&mut self, platform: &Platform, _alpha: f32) {
        self.draw_sprite(platform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeLoopConfig, block_on_loop};
    use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};

    #[test]
    fn demo_host_advances_frames_headless() {
        let mut platform = Platform::new(
            Box::new(NullPlatform::new().with_frames(std::iter::repeat_n(NullFrame::idle(), 4))),
            Box::new(FixedPaths::under("test")),
            PlatformConfig::default(),
        );
        let (mut host, log) = AnimationDemoHost::headless().expect("demo");
        let _ = block_on_loop(
            &mut platform,
            &mut host,
            &RuntimeLoopConfig {
                max_ticks: Some(8),
                max_frames: Some(4),
                ..RuntimeLoopConfig::default()
            },
        );
        assert_eq!(log.draws().len(), 4);
        assert!(log.draws()[0].src.x <= log.draws()[3].src.x);
    }
}
