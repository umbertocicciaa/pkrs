//! CLI test (§2.3, §13): `pkrs pack` writes a `game.pak` that opens through `PakVfs`, with
//! dev-only files left out and tileset/sprite images moved into atlases. (Running the game
//! from the archive is covered in `pkrs-runtime/tests/pak_headless.rs`.)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use pkrs_assets::{PakVfs, Vfs, VfsPath};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pkrs(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pkrs"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn pack_writes_a_checked_archive_with_atlases() {
    let root = temp_dir("pack");
    let new = pkrs(&["new", "demo"], &root);
    assert!(
        new.status.success(),
        "{}",
        String::from_utf8_lossy(&new.stderr)
    );
    let project = root.join("demo");
    std::fs::create_dir_all(project.join("saves")).unwrap();
    std::fs::write(project.join("saves/slot1.ron"), "()").unwrap();

    let out = pkrs(&["pack"], &project);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.starts_with("Packed com.example.demo 0.1.0"),
        "{stdout}"
    );

    let pak = PakVfs::open(&project.join("dist/game.pak")).unwrap();
    let path = |p: &str| VfsPath::new(p).unwrap();
    assert!(pak.exists(&path("pkrs.toml")));
    assert!(pak.exists(&path("maps/start.tmx")));
    assert!(
        !pak.exists(&path("saves/slot1.ron")),
        "dev saves do not ship"
    );
    assert!(!pak.exists(&path(".gitignore")), "hidden files do not ship");
    let index = pak.index();
    assert!(index.atlas_rect(&path("sprites/player.png")).is_some());
    assert!(
        !pak.exists(&path("sprites/player.png")),
        "packed images are not files"
    );
    for page in &index.atlases {
        assert!(pak.read(&path(&page.path)).unwrap().starts_with(b"\x89PNG"));
    }

    // Packing again gives the same bytes; `--out` writes elsewhere.
    let again = pkrs(&["pack", "--out", "../other.pak"], &project);
    assert!(again.status.success());
    assert_eq!(
        std::fs::read(project.join("dist/game.pak")).unwrap(),
        std::fs::read(root.join("other.pak")).unwrap()
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn pack_outside_a_project_fails() {
    let root = temp_dir("pack-none");
    let out = pkrs(&["pack"], &root);
    assert!(!out.status.success());
    std::fs::remove_dir_all(&root).unwrap();
}
