//! Headless integration: simulation ticks drive animated sprite src rects in `NullRenderer`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{
    DisplaySettings, FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig,
};
use pkrs_render::{
    ClipPlayer, Layer, NullRenderer, Rect, Renderer, RendererConfig, SpriteDraw, TextureHandle,
    clip_for_direction, parse_animation_descriptor,
};
use pkrs_runtime::{RuntimeLoopConfig, SimulationHost, block_on_loop};

const DEMO_RON: &str = include_str!("fixtures/demo_mascot.ron");

struct AnimatedSpriteHost {
    renderer: Renderer,
    log: pkrs_render::DrawLog,
    player: ClipPlayer,
    sheet: pkrs_render::SpriteSheetAnimation,
    texture: TextureHandle,
}

impl AnimatedSpriteHost {
    fn new() -> Self {
        let sheet = parse_animation_descriptor(DEMO_RON).expect("demo descriptor");
        let resolved = clip_for_direction(&sheet, "walk", "down").expect("walk_down");
        let null = NullRenderer::new();
        let log = null.draw_log();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        let texture = renderer
            .create_texture(64, 32, &[0; 64 * 32 * 4])
            .expect("texture");
        Self {
            player: ClipPlayer::new(resolved.clip),
            sheet,
            texture,
            renderer,
            log,
        }
    }
}

impl SimulationHost for AnimatedSpriteHost {
    fn poll_input(&mut self, _platform: &Platform) {}

    fn begin_tick(&mut self) {}

    fn simulation_tick(&mut self, _dt: f64) {
        self.player.tick();
    }

    fn render(&mut self, _platform: &Platform, _alpha: f32) {
        self.renderer.begin_frame(240, 160);
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

#[test]
fn headless_ticks_select_expected_animation_frame_in_draw_list() {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(std::iter::repeat_n(NullFrame::idle(), 32))),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );
    let mut host = AnimatedSpriteHost::new();
    let log = host.log.clone();
    let ticks_per_frame = host.sheet.clip("walk_down").expect("clip").ticks_per_frame;

    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: None,
            max_frames: Some(8),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, pkrs_runtime::LoopExit::FrameLimit);

    let draws = log.draws();
    assert_eq!(draws.len(), 8);
    let expected_src_x = |tick: u64| -> f32 {
        let pos = pkrs_render::frame_pos_after_ticks(
            host.sheet.clip("walk_down").expect("clip"),
            tick.saturating_add(1),
        );
        let frame = host.sheet.clip("walk_down").expect("clip").frames[pos];
        host.sheet.frame_src_rect(frame).x
    };
    for (i, draw) in draws.iter().enumerate() {
        assert_eq!(
            draw.src.x,
            expected_src_x(u64::try_from(i).expect("index")),
            "draw {i} src.x"
        );
    }
    assert_eq!(ticks_per_frame, 3);
}
