//! Headless integration: bitmap text emits one sprite draw per glyph.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::DisplaySettings;
use pkrs_render::{Layer, NullRenderer, Renderer, RendererConfig, TextStyle, parse_glyph_map_ron};

#[test]
fn headless_text_records_glyph_draws() {
    let font = parse_glyph_map_ron(include_str!("fixtures/minimal.ron")).expect("font");
    let null = NullRenderer::new();
    let log = null.draw_log();
    let mut renderer = Renderer::new(
        Box::new(null),
        DisplaySettings::default(),
        RendererConfig::default(),
    );
    let texture = renderer
        .create_texture(font.atlas_width, font.atlas_height, &[])
        .expect("tex");

    renderer.begin_frame(320, 240);
    renderer.draw_text(&font, texture, "A", 10.0, 20.0, TextStyle::default());
    renderer.end_frame();

    assert_eq!(log.draws().len(), 1);
    assert_eq!(log.draws()[0].layer, Layer::Ui);
    assert_eq!(log.draws()[0].dst.x, 10.0);
    assert_eq!(log.draws()[0].dst.y, 20.0);
}

#[test]
fn two_character_string_two_draws() {
    let font = parse_glyph_map_ron(include_str!("fixtures/minimal.ron")).expect("font");
    let null = NullRenderer::new();
    let log = null.draw_log();
    let mut renderer = Renderer::new(
        Box::new(null),
        DisplaySettings::default(),
        RendererConfig::default(),
    );
    let texture = renderer
        .create_texture(font.atlas_width, font.atlas_height, &[])
        .expect("tex");
    renderer.begin_frame(320, 240);
    renderer.draw_text(&font, texture, "AB", 0.0, 0.0, TextStyle::default());
    renderer.end_frame();
    assert_eq!(log.draws().len(), 2);
    assert!(log.draws()[1].dst.x > log.draws()[0].dst.x);
}
