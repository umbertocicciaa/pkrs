//! Headless integration: one frame through the `Renderer` facade and `NullRenderer`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::DisplaySettings;
use pkrs_render::{Layer, NullRenderer, Rect, Renderer, RendererConfig, SpriteDraw, TextureHandle};

#[test]
fn headless_frame_records_layered_sprite() {
    let null = NullRenderer::new();
    let log = null.draw_log();
    let mut renderer = Renderer::new(
        Box::new(null),
        DisplaySettings::default(),
        RendererConfig::default(),
    );

    renderer.begin_frame(720, 480);
    renderer.draw_sprite(SpriteDraw {
        texture: TextureHandle(7),
        src: Rect::new(0.0, 0.0, 16.0, 16.0),
        dst: Rect::new(100.0, 50.0, 16.0, 16.0),
        flip_x: false,
        layer: Layer::Entities,
    });
    renderer.end_frame();

    assert_eq!(log.draws().len(), 1);
    assert_eq!(log.draws()[0].texture, TextureHandle(7));
    assert_eq!(log.draws()[0].layer, Layer::Entities);
    assert_eq!(log.draws()[0].dst.x, 100.0);
}

#[test]
fn multiple_frames_accumulate_in_log_when_not_cleared() {
    let null = NullRenderer::new();
    let log = null.draw_log();
    let mut renderer = Renderer::new(
        Box::new(null),
        DisplaySettings::default(),
        RendererConfig::default(),
    );

    for _ in 0..3 {
        renderer.begin_frame(320, 240);
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(1),
            src: Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            layer: Layer::Ground,
        });
        renderer.end_frame();
    }

    assert_eq!(log.draws().len(), 3);
}
