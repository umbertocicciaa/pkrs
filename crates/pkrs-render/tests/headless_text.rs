//! Headless integration: bitmap text emits one sprite draw per glyph.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::DisplaySettings;
use pkrs_render::{
    Color, Layer, NullRenderer, Renderer, RendererConfig, TextStyle, parse_glyph_map_ron,
};

#[test]
fn headless_text_records_glyph_draws() {
    let font = parse_glyph_map_ron(include_str!("fixtures/minimal.ron")).expect("font");
    let glyph_a = font.glyph('A'.into()).expect("A glyph");
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

    let style = TextStyle {
        tint: Color {
            r: 200,
            g: 100,
            b: 50,
            a: 255,
        },
        ..TextStyle::default()
    };
    renderer.begin_frame(320, 240);
    renderer.draw_text(&font, texture, "A", 10.0, 20.0, style);
    renderer.end_frame();

    assert_eq!(log.draws().len(), 1);
    assert_eq!(log.draws()[0].layer, Layer::Ui);
    assert_eq!(log.draws()[0].tint, style.tint);
    assert_eq!(log.draws()[0].dst.x, 10.0 + glyph_a.xoffset as f32);
    assert_eq!(log.draws()[0].dst.y, 20.0 + glyph_a.yoffset as f32);
    assert_eq!(log.draws()[0].dst.width, glyph_a.src.width);
    assert_eq!(log.draws()[0].dst.height, glyph_a.src.height);
}

#[test]
fn two_character_string_two_draws() {
    let font = parse_glyph_map_ron(include_str!("fixtures/minimal.ron")).expect("font");
    let glyph_a = font.glyph('A'.into()).expect("A");
    let glyph_b = font.glyph('B'.into()).expect("B");
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
    let second_x = glyph_a.xoffset as f32 + font.advance(Some('A'), 'B');
    assert_eq!(log.draws()[1].dst.x, second_x + glyph_b.xoffset as f32);
}
