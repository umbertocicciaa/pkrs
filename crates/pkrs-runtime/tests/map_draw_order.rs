//! Integration (§5.2): a headless render of a fixture Tiled map draws layers in order
//! `Ground → GroundDecor → Entities (y-sorted) → Overhead`, recorded by `NullRenderer`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pkrs_assets::{FolderVfs, VfsPath};
use pkrs_core::GridPos;
use pkrs_platform::DisplaySettings;
use pkrs_render::{DrawLog, Layer, NullRenderer, Rect, Renderer, RendererConfig, TextureHandle};
use pkrs_runtime::{MapScene, PLAYER_ENTITY};
use pkrs_world::{MapImporter, ObjectKind, TiledImporter};

fn load_town() -> pkrs_world::Map {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pkrs-world/tests/fixtures/project");
    let vfs = FolderVfs::new(root).unwrap();
    TiledImporter::new()
        .import(&vfs, &VfsPath::new("maps/town.tmx").unwrap())
        .unwrap()
}

fn headless_renderer(display: DisplaySettings) -> (Renderer, DrawLog) {
    let null = NullRenderer::new();
    let log = null.draw_log();
    (
        Renderer::new(Box::new(null), display, RendererConfig::default()),
        log,
    )
}

const TILESET: TextureHandle = TextureHandle(10);
const PLAYER: TextureHandle = TextureHandle(20);
const NPC: TextureHandle = TextureHandle(30);

#[test]
fn fixture_map_draws_in_layer_and_y_order() {
    let map = load_town();
    let npcs: Vec<_> = map
        .objects
        .iter()
        .filter(|o| matches!(o.kind, ObjectKind::Npc { .. }))
        .map(|o| (o.id, o.pos()))
        .collect();
    let spawn = map.spawn("default").unwrap().pos();

    let mut scene = MapScene::new(map, &[TILESET]).unwrap();
    let sprite = Rect::new(0.0, 0.0, 16.0, 16.0);
    scene.place_entity(PLAYER_ENTITY, spawn, PLAYER, sprite, false);
    for (id, pos) in &npcs {
        scene.place_entity(*id, *pos, NPC, sprite, false);
    }
    // A second NPC on the player's row, to exercise the stable-id tiebreak.
    scene.place_entity(99, GridPos::new(spawn.x + 1, spawn.y), NPC, sprite, false);

    let (mut renderer, log) = headless_renderer(DisplaySettings::default());
    renderer.begin_frame(240, 160);
    let origin = scene.render(&mut renderer, 0.0, &[]);
    renderer.end_frame();

    // The 96×64 town is smaller than the 240×160 view, so it is centred.
    assert_eq!(origin, (-72, -48));

    let draws = log.draws();
    let layers: Vec<Layer> = draws.iter().map(|d| d.layer).collect();
    let mut bands = layers.clone();
    bands.dedup();
    assert_eq!(
        bands,
        [
            Layer::Ground,
            Layer::GroundDecor,
            Layer::Entities,
            Layer::Overhead
        ],
        "bands are contiguous and ordered"
    );
    assert_eq!(
        layers.iter().filter(|&&l| l == Layer::Ground).count(),
        24,
        "6×4 ground tiles"
    );
    assert_eq!(
        layers.iter().filter(|&&l| l == Layer::GroundDecor).count(),
        1
    );
    assert_eq!(layers.iter().filter(|&&l| l == Layer::Overhead).count(), 1);

    // Ground tiles in row-major order, positioned relative to the centred origin.
    let first = &draws[0];
    assert_eq!((first.dst.x, first.dst.y), (72.0, 48.0));
    assert_eq!(first.texture, TILESET);

    // Entities: player (row 2) and NPC 99 (row 2) before the keeper NPC (row 3);
    // on the same row the stable id decides (player id 0 < 99).
    let entities: Vec<_> = draws
        .iter()
        .filter(|d| d.layer == Layer::Entities)
        .map(|d| (d.texture, d.dst.x, d.dst.y))
        .collect();
    assert_eq!(
        entities,
        [
            (PLAYER, 72.0 + 32.0, 48.0 + 32.0),
            (NPC, 72.0 + 48.0, 48.0 + 32.0),
            (NPC, 72.0 + 32.0, 48.0 + 48.0),
        ]
    );
}

#[test]
fn same_row_order_is_stable_across_frames() {
    let mut scene = MapScene::new(load_town(), &[TILESET]).unwrap();
    let sprite = Rect::new(0.0, 0.0, 16.0, 16.0);
    for id in [7, 3, 5] {
        scene.place_entity(id, GridPos::new(id as i32 % 6, 1), NPC, sprite, false);
    }
    let (mut renderer, log) = headless_renderer(DisplaySettings::default());
    let mut orders = Vec::new();
    for _ in 0..3 {
        renderer.begin_frame(240, 160);
        scene.render(&mut renderer, 0.5, &[]);
        renderer.end_frame();
        let xs: Vec<_> = log
            .draws()
            .iter()
            .filter(|d| d.layer == Layer::Entities)
            .map(|d| d.dst.x)
            .collect();
        orders.push(xs);
        log.clear();
    }
    assert!(orders.windows(2).all(|w| w[0] == w[1]), "{orders:?}");
}
