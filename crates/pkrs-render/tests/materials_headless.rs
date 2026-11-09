//! Integration (§5.6): `NullRenderer` records material creation and, for every draw, the
//! bound material with its resolved uniforms (engine, global and constant values).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::DisplaySettings;
use pkrs_render::{
    Color, DrawLog, EntitySprite, Layer, Material, NullRenderer, Rect, Renderer, RendererConfig,
    SpriteDraw, TileGrid, TileInstance, TileMaterials, TileSheet, UniformType, UniformValue,
};

const WATER: &str = r#"(
    shader: "pkrs/animated_tiles",
    uniforms: {
        "frames": Int(2),
        "ticks_per_frame": Int(10),
        "tint": Global(name: "day_tint"),
    },
    atlas_region: (x: 0, y: 16, width: 32, height: 16),
)"#;

const DAY_NIGHT: &str = r#"(
    shader: "pkrs/tint",
    uniforms: { "tint": Global(name: "day_tint", default: Color(255, 255, 255, 255)) },
)"#;

/// 2×1 layer: a grass tile (id 0, row 0) and a water tile (id 2, row 1).
struct Strip;

impl TileGrid for Strip {
    fn size(&self) -> (u32, u32) {
        (2, 1)
    }
    fn tile_at(&self, x: u32, _: u32) -> Option<TileInstance> {
        Some(TileInstance {
            sheet: 0,
            id: if x == 0 { 0 } else { 2 },
            flip_x: false,
        })
    }
}

fn setup() -> (Renderer, DrawLog, TileSheet) {
    let null = NullRenderer::new();
    let log = null.draw_log();
    let mut renderer = Renderer::new(
        Box::new(null),
        DisplaySettings::default(),
        RendererConfig::default(),
    );
    let texture = renderer.create_texture(32, 32, &[0; 32 * 32 * 4]).unwrap();
    let sheet = TileSheet {
        texture,
        tile_width: 16,
        tile_height: 16,
        columns: 2,
        tile_count: 4,
        margin: 0,
        spacing: 0,
    };
    (renderer, log, sheet)
}

fn no_files(_: &str) -> Option<String> {
    None
}

#[test]
fn null_renderer_records_material_binds_over_ticks() {
    let (mut renderer, log, sheet) = setup();
    let water = renderer
        .create_material("water", Material::from_ron(WATER, &no_files).unwrap())
        .unwrap();
    let day_night = renderer
        .create_material(
            "day_night",
            Material::from_ron(DAY_NIGHT, &no_files).unwrap(),
        )
        .unwrap();
    {
        let created = log.materials();
        assert_eq!(created.len(), 2);
        assert_eq!(created[0].name, "water");
        assert_eq!(
            created[0].uniforms,
            [
                ("frame".to_owned(), UniformType::Float),
                ("frame_offset".to_owned(), UniformType::Vec2),
                ("texture_size".to_owned(), UniformType::Vec2),
                ("tint".to_owned(), UniformType::Vec4),
            ]
        );
    }

    let materials = TileMaterials {
        layer: Some(day_night),
        sheets: &[Some(water)],
    };
    let mut frames = Vec::new();
    for tick in [0u64, 10, 20] {
        if tick == 20 {
            renderer.set_global_uniform("day_tint", UniformValue::Vec4([0.4, 0.4, 0.7, 1.0]));
        }
        renderer.set_time_ticks(tick);
        renderer.begin_frame(240, 160);
        let queued = renderer.draw_tile_layer_with(
            &Strip,
            &[sheet],
            (16, 16),
            Layer::Ground,
            (-8, 0),
            materials,
        );
        assert_eq!(queued, 2);
        renderer.end_frame();
        let draws = log.draws();
        // Grass: outside the water region, so the layer's day/night tint applies.
        let grass = draws[0].material.as_ref().unwrap();
        assert_eq!(grass.material, day_night);
        // Water: the tileset material wins inside its atlas region.
        let water_bind = draws[1].material.as_ref().unwrap();
        assert_eq!(water_bind.material, water);
        assert_eq!(
            water_bind.uniform("texture_size"),
            Some(UniformValue::Vec2([32.0, 32.0]))
        );
        assert_eq!(
            water_bind.uniform("frame_offset"),
            Some(UniformValue::Vec2([16.0, 0.0]))
        );
        frames.push((
            water_bind.uniform("frame"),
            water_bind.uniform("tint"),
            grass.uniform("tint"),
        ));
        drop(draws);
        log.clear();
    }
    let night = Some(UniformValue::Vec4([0.4, 0.4, 0.7, 1.0]));
    let white = Some(UniformValue::Vec4([1.0; 4]));
    assert_eq!(
        frames,
        [
            (Some(UniformValue::Float(0.0)), white, white),
            (Some(UniformValue::Float(1.0)), white, white),
            (Some(UniformValue::Float(0.0)), night, night),
        ]
    );
}

#[test]
fn sprites_and_entities_bind_materials() {
    let (mut renderer, log, sheet) = setup();
    let ghost = renderer
        .create_material(
            "ghost",
            Material::from_ron(
                r#"(shader: "pkrs/translucency", uniforms: { "alpha": Float(0.25) })"#,
                &no_files,
            )
            .unwrap(),
        )
        .unwrap();
    let draw = SpriteDraw {
        texture: sheet.texture,
        src: Rect::new(0.0, 0.0, 16.0, 16.0),
        dst: Rect::new(0.0, 0.0, 16.0, 16.0),
        flip_x: false,
        tint: Color::WHITE,
        layer: Layer::Entities,
    };
    renderer.begin_frame(240, 160);
    renderer.set_map_origin((4.0, 2.0));
    renderer.draw_sprite_with_material(draw, Some(ghost));
    renderer.draw_sprite(draw);
    let entities = [
        EntitySprite {
            id: 2,
            feet_y: 40.0,
            draw,
        },
        EntitySprite {
            id: 1,
            feet_y: 20.0,
            draw,
        },
    ];
    renderer.draw_entities_with(&entities, &[Some(ghost), None]);
    renderer.end_frame();
    let draws = log.draws();
    let binds: Vec<_> = draws
        .iter()
        .map(|d| d.material.as_ref().map(|b| b.uniform("alpha")))
        .collect();
    assert_eq!(
        binds,
        [
            Some(Some(UniformValue::Float(0.25))),
            None,
            None,                                  // id 1 (feet 20) first, no material
            Some(Some(UniformValue::Float(0.25))), // id 2
        ]
    );
}

#[test]
fn unknown_material_handles_draw_plainly() {
    let (mut renderer, log, sheet) = setup();
    renderer.begin_frame(240, 160);
    renderer.draw_tile_layer_with(
        &Strip,
        &[sheet],
        (16, 16),
        Layer::Ground,
        (0, 0),
        TileMaterials {
            layer: Some(pkrs_render::MaterialHandle(99)),
            sheets: &[],
        },
    );
    renderer.end_frame();
    assert!(log.draws().iter().all(|d| d.material.is_none()));
}
