//! Integration: load fixture `.tmx` + `.tsx` files from a project folder through `FolderVfs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pkrs_assets::{FolderVfs, VfsPath};
use pkrs_core::{Direction, GridPos};
use pkrs_world::{
    MapError, MapImporter, ObjectKind, Terrain, TileRect, TiledImporter, Transition, ViolationKind,
    VisualLayerKind, Warp,
};

fn project() -> FolderVfs {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project");
    FolderVfs::new(root).expect("fixture project")
}

fn import(path: &str) -> Result<pkrs_world::Map, MapError> {
    TiledImporter::new().import(&project(), &VfsPath::new(path).unwrap())
}

#[test]
fn town_loads_visual_layers_in_order() {
    let map = import("maps/town.tmx").unwrap();
    assert_eq!(map.id, "town");
    assert_eq!((map.width, map.height), (6, 4));
    assert_eq!(map.pixel_size(), (96, 64));
    let layers: Vec<_> = map
        .layers
        .iter()
        .map(|l| (l.name.as_str(), l.kind))
        .collect();
    assert_eq!(
        layers,
        [
            ("ground", VisualLayerKind::Ground),
            ("ground2", VisualLayerKind::GroundDecor),
            ("overhead", VisualLayerKind::Overhead),
        ]
    );
    let image = map.tilesets[0].image.as_ref().unwrap();
    assert_eq!(image.path, "tilesets/town.png");
    assert_eq!((image.width, image.height), (64, 32));
    assert_eq!(map.properties.string("bgm"), Ok(Some("town_theme")));
}

#[test]
fn town_logical_grid_has_collision_edges_and_terrain() {
    let map = import("maps/town.tmx").unwrap();
    let grid = &map.grid;
    assert!(
        grid.is_solid(GridPos::new(0, 0)),
        "collision tile with solid=true"
    );
    assert!(
        !grid.is_solid(GridPos::new(2, 2)),
        "edge-only collision tile"
    );
    assert!(
        !grid.can_cross(GridPos::new(2, 2), Direction::Right),
        "block_e"
    );
    assert!(
        !grid.can_cross(GridPos::new(3, 2), Direction::Left),
        "block_e from the other side"
    );
    assert!(
        !grid.can_cross(GridPos::new(3, 3), Direction::Right),
        "block_e on ground2"
    );
    assert_eq!(grid.terrain(GridPos::new(1, 1)), Some(&Terrain::TallGrass));
    assert_eq!(grid.terrain(GridPos::new(5, 2)), Some(&Terrain::Water));
    assert_eq!(
        grid.terrain(GridPos::new(0, 3)),
        Some(&Terrain::Ledge(Direction::Down))
    );
    assert_eq!(grid.terrain(GridPos::new(3, 0)), None);
}

#[test]
fn town_has_every_object_kind() {
    let map = import("maps/town.tmx").unwrap();
    let classes: Vec<_> = map.objects.iter().map(|o| o.kind.class_name()).collect();
    assert_eq!(classes, ["warp", "npc", "trigger", "sign", "item", "spawn"]);

    let door = map.object(1).unwrap();
    assert_eq!(door.pos(), GridPos::new(5, 0));
    assert_eq!(
        door.kind,
        ObjectKind::Warp(Warp {
            to_map: "house".into(),
            to: GridPos::new(3, 6),
            facing: Some(Direction::Up),
            transition: Transition::Door,
        })
    );
    assert_eq!(
        map.object(2).unwrap().kind,
        ObjectKind::Npc {
            script: Some("maps/town.lua:old_keeper".into()),
            sprite: Some("sprites/keeper.png".into()),
            facing: Direction::Left,
        }
    );
    let trigger = map.object(3).unwrap();
    assert_eq!(trigger.rect, TileRect::new(0, 1, 1, 2));
    assert_eq!(
        trigger.kind,
        ObjectKind::Trigger {
            on_enter: Some("cutscenes/rival_intro".into()),
            once: true,
        }
    );
    assert_eq!(
        map.object(4).unwrap().kind,
        ObjectKind::Sign {
            script: None,
            text: Some("text:town.sign".into()),
        }
    );
    let item = map.object(5).unwrap();
    assert_eq!(
        item.kind,
        ObjectKind::Item {
            item: "potion".into(),
            count: 2,
        }
    );
    assert_eq!(item.persistent_key(), "town_potion");
    let spawn = map.spawn("default").unwrap();
    assert_eq!(spawn.pos(), GridPos::new(2, 2));
    assert_eq!(spawn.rect, TileRect::new(2, 2, 1, 1));
}

#[test]
fn town_encounter_rects() {
    let map = import("maps/town.tmx").unwrap();
    assert_eq!(map.encounters.len(), 1);
    let zone = &map.encounters[0];
    assert_eq!(zone.table, "town_grass");
    assert_eq!(zone.rate, 20);
    assert_eq!(zone.rect, TileRect::new(1, 1, 2, 1));
    assert_eq!(map.encounters_at(GridPos::new(2, 1)).count(), 1);
    assert_eq!(map.encounters_at(GridPos::new(3, 1)).count(), 0);
}

#[test]
fn broken_map_reports_every_violation() {
    let err = import("maps/broken.tmx").unwrap_err();
    let kinds: Vec<_> = err.violations().iter().map(|v| v.kind.clone()).collect();
    assert_eq!(
        kinds,
        [
            ViolationKind::UnknownLayer { kind: "tile" },
            ViolationKind::MissingProperty("to_map"),
            ViolationKind::MissingProperty("to_x"),
            ViolationKind::MissingProperty("to_y"),
            ViolationKind::UnknownObjectClass("phantom".into()),
            ViolationKind::MissingLayer("ground"),
        ],
        "{err}"
    );
}

#[test]
fn missing_map_is_a_read_error() {
    assert!(matches!(
        import("maps/nowhere.tmx"),
        Err(MapError::Read { .. })
    ));
}
