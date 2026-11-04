//! The same asset set loaded from a fixture project through `FolderVfs` and, copied into
//! memory, through `MemoryVfs` must give identical results via the `Assets` facade (§13).

// Test-only helpers outside `#[test]` fns are not covered by clippy.toml's test exemption.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use pkrs_assets::{
    Asset, AssetPath, Assets, AssetsError, EntryKind, FolderVfs, Handle, MemoryVfs, Result, Text,
};

/// Minimal texture stand-in: dimensions parsed from the PNG `IHDR` chunk.
/// Real decoding belongs to `pkrs-render`; this proves typed decoding through the cache.
#[derive(Debug, PartialEq, Eq)]
struct PngInfo {
    width: u32,
    height: u32,
    bytes: usize,
}

impl Asset for PngInfo {
    fn decode(bytes: Vec<u8>, path: &AssetPath) -> Result<Self> {
        const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
        let be = |at: usize| -> Option<u32> {
            Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        };
        match (
            bytes.starts_with(SIGNATURE),
            bytes.get(12..16),
            be(16),
            be(20),
        ) {
            (true, Some(b"IHDR"), Some(width), Some(height)) => Ok(Self {
                width,
                height,
                bytes: bytes.len(),
            }),
            _ => Err(AssetsError::Decode {
                path: path.to_string(),
                message: "not a PNG".to_owned(),
            }),
        }
    }
}

const TEXTURES: &[&str] = &[
    "assets/sprites/hero.png",
    "assets/tilesets/town.png",
    "resonance:assets/ui/icon.png",
];
const RON: &[&str] = &[
    "data/types.ron",
    "data/species/emberkit.ron",
    "resonance:data/moves/resonate.ron",
];

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

/// Copies every file under `dir` into a `MemoryVfs`, keyed by its relative path.
fn memory_copy(dir: &Path) -> MemoryVfs {
    fn walk(base: &Path, dir: &Path, vfs: &mut MemoryVfs) {
        for entry in fs::read_dir(dir).expect("read fixture dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                walk(base, &path, vfs);
            } else {
                let rel = path.strip_prefix(base).expect("inside base");
                let rel = rel.to_str().expect("utf-8 fixture path");
                vfs.insert(rel, fs::read(&path).expect("read fixture file"))
                    .expect("valid path");
            }
        }
    }
    let mut vfs = MemoryVfs::new();
    walk(dir, dir, &mut vfs);
    vfs
}

fn folder_assets() -> Assets {
    let root = fixture();
    let mut assets = Assets::new(FolderVfs::new(&root).expect("open project"));
    assets
        .mount(
            "resonance",
            FolderVfs::new(root.join("plugins/resonance")).expect("open plugin"),
        )
        .expect("mount");
    assets
}

fn memory_assets() -> Assets {
    let root = fixture();
    let mut assets = Assets::new(memory_copy(&root));
    assets
        .mount("resonance", memory_copy(&root.join("plugins/resonance")))
        .expect("mount");
    assets
}

/// Loads every fixture asset and returns the decoded values, in order.
fn load_all(assets: &mut Assets) -> (Vec<(u32, u32, usize)>, Vec<String>) {
    let textures = TEXTURES
        .iter()
        .map(|p| {
            let h: Handle<PngInfo> = assets.load(p).expect("texture loads");
            let info = assets.require(h).expect("cached");
            (info.width, info.height, info.bytes)
        })
        .collect();
    let ron = RON
        .iter()
        .map(|p| {
            let h: Handle<Text> = assets.load(p).expect("ron loads");
            assets.require(h).expect("cached").0.clone()
        })
        .collect();
    (textures, ron)
}

#[test]
fn folder_and_memory_load_identical_assets() {
    let mut folder = folder_assets();
    let mut memory = memory_assets();
    let from_folder = load_all(&mut folder);
    let from_memory = load_all(&mut memory);
    assert_eq!(from_folder, from_memory);

    assert_eq!(
        from_folder.0.iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(),
        [(16, 32), (64, 48), (8, 8)]
    );
    assert!(from_folder.1[1].contains("\"emberkit\""));
    assert_eq!(folder.cached_count(), 6);
    assert_eq!(memory.cached_count(), 6);
}

#[test]
fn folder_and_memory_list_identically() {
    let folder = folder_assets();
    let memory = memory_assets();
    for dir in [
        "",
        "assets",
        "data/species",
        "resonance:",
        "resonance:data/moves",
    ] {
        assert_eq!(
            folder.list(dir).expect("folder list"),
            memory.list(dir).expect("memory list"),
            "listing `{dir}`"
        );
    }
    let root = folder.list("").expect("root");
    assert!(
        root.iter()
            .any(|e| e.path.to_string() == "pkrs.toml" && e.kind == EntryKind::File)
    );
    assert!(
        root.iter()
            .any(|e| e.path.to_string() == "plugins" && e.kind == EntryKind::Dir)
    );
}

#[test]
fn traversal_outside_the_project_is_rejected_by_both() {
    for assets in [folder_assets(), memory_assets()] {
        for raw in [
            "../Cargo.toml",
            "assets/../../Cargo.toml",
            "resonance:../../pkrs.toml",
            "/etc/passwd",
        ] {
            let err = assets.read(raw).expect_err(raw);
            assert!(
                matches!(
                    err,
                    AssetsError::PathEscapesRoot(_) | AssetsError::InvalidPath { .. }
                ),
                "{raw}: {err:?}"
            );
            assert!(!assets.exists(raw));
        }
        // `..` that stays inside the root is fine.
        assert!(assets.exists("assets/sprites/../tilesets/town.png"));
    }
}

#[test]
fn wrong_case_is_rejected_by_both() {
    for assets in [folder_assets(), memory_assets()] {
        assert!(matches!(
            assets.read("Assets/Sprites/hero.png"),
            Err(AssetsError::CaseMismatch { .. })
        ));
    }
}

#[test]
fn decode_errors_surface_with_the_path() {
    let mut assets = folder_assets();
    match assets.load::<PngInfo>("data/types.ron") {
        Err(AssetsError::Decode { path, .. }) => assert_eq!(path, "data/types.ron"),
        other => panic!("expected a decode error, got {other:?}"),
    }
    assert_eq!(assets.cached_count(), 0);
}
