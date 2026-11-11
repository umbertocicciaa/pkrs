//! Integration (§13, §5.2): fixture projects packed with `pkrs pack` (atlases included) run
//! from `game.pak` exactly as they do from their folders: the headless scenario suite passes
//! against every archive, and a frame draws the same sprites at the same places, only moved
//! onto atlas pages.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use pkrs_assets::PakIndex;
use pkrs_cli::backend::TexturePackerAdapter;
use pkrs_cli::pack::{PackReport, pack_project};
use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_render::{DrawLog, NullRenderer, RecordedDraw, Renderer, RendererConfig, TextureHandle};
use pkrs_runtime::project::{Project, ProjectError, ProjectHost, ProjectOptions};
use pkrs_runtime::test_harness::{ScenarioOptions, discover_scenarios, run_project_scenarios};
use pkrs_runtime::{RuntimeLoopConfig, block_on_loop};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/projects")
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-pak-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pack(project: &Path, out_dir: &Path) -> PackReport {
    let out = out_dir.join(format!(
        "{}.pak",
        project.file_name().unwrap().to_string_lossy()
    ));
    pack_project(project, &out, &TexturePackerAdapter).unwrap()
}

fn open_folder(dir: &Path) -> Project {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        ..ProjectOptions::default()
    })
    .unwrap()
}

fn open_pak(dir: &Path, pak: &Path) -> Result<Project, ProjectError> {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        pak: Some(pak.to_path_buf()),
        ..ProjectOptions::default()
    })
}

#[test]
fn every_fixture_scenario_passes_against_its_pak() {
    let out = temp_dir("scenarios");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixture_root())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !discover_scenarios(p).unwrap().is_empty())
        .collect();
    dirs.sort();
    let mut ran = 0;
    let mut failures = Vec::new();
    for dir in &dirs {
        let report = pack(dir, &out);
        assert!(
            report
                .index
                .files
                .iter()
                .all(|f| !f.path.starts_with("tests/")),
            "scenarios do not ship"
        );
        assert!(
            !report.index.sprites.is_empty(),
            "{}: tilesets are packed into atlases",
            dir.display()
        );
        let project = open_pak(dir, &report.out).unwrap();
        for scenario in run_project_scenarios(&project, dir, &ScenarioOptions::default()).unwrap() {
            ran += 1;
            if !scenario.passed() {
                failures.push(format!("{} (pak): {scenario}", dir.display()));
            }
        }
    }
    assert!(ran >= 10, "expected every fixture suite, ran {ran}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    std::fs::remove_dir_all(&out).unwrap();
}

/// Draws of `ticks` idle frames of a fresh host on `project`.
fn draws(project: &Project, ticks: u32) -> Vec<RecordedDraw> {
    let null = NullRenderer::new();
    let log: DrawLog = null.draw_log();
    let renderer = Renderer::new(Box::new(null), project.display(), RendererConfig::default());
    let mut host = ProjectHost::new(project, renderer).unwrap();
    let frames = std::iter::repeat_n(NullFrame::idle(), ticks as usize);
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under("pkrs-pak-test")),
        PlatformConfig::default(),
    );
    block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(u64::from(ticks)),
            ..RuntimeLoopConfig::default()
        },
    );
    log.draws().to_vec()
}

/// Asserts that `pak` draws are `folder` draws with each texture moved onto an atlas page at
/// one of the index's rects, consistently per texture.
fn assert_same_frames(folder: &[RecordedDraw], pak: &[RecordedDraw], index: &PakIndex) {
    assert_eq!(folder.len(), pak.len(), "same number of draws");
    assert!(!folder.is_empty());
    let origins: Vec<(f32, f32)> = index
        .sprites
        .values()
        .map(|r| (r.x as f32, r.y as f32))
        .collect();
    let mut mapping: BTreeMap<TextureHandle, (TextureHandle, f32, f32)> = BTreeMap::new();
    let mut moved = 0;
    for (i, (f, p)) in folder.iter().zip(pak).enumerate() {
        assert_eq!(f.is_filled_rect, p.is_filled_rect, "draw {i}");
        assert_eq!(f.layer, p.layer, "draw {i}");
        assert_eq!(f.dst, p.dst, "draw {i}");
        assert_eq!(f.flip_x, p.flip_x, "draw {i}");
        assert_eq!(f.tint, p.tint, "draw {i}");
        assert_eq!(f.material, p.material, "draw {i}: same uniforms");
        if f.is_filled_rect {
            continue;
        }
        assert_eq!(
            (f.src.width, f.src.height),
            (p.src.width, p.src.height),
            "draw {i}"
        );
        let offset = (p.src.x - f.src.x, p.src.y - f.src.y);
        let seen = *mapping
            .entry(f.texture)
            .or_insert((p.texture, offset.0, offset.1));
        assert_eq!(
            seen,
            (p.texture, offset.0, offset.1),
            "draw {i}: one place per texture"
        );
        if offset != (0.0, 0.0) || origins.contains(&offset) {
            assert!(
                origins.contains(&offset),
                "draw {i}: {offset:?} is an atlas rect"
            );
            moved += 1;
        }
    }
    assert!(moved > 0, "some draws come from atlas pages");
}

#[test]
fn a_new_project_draws_the_same_frames_from_folder_and_pak() {
    let dir = temp_dir("new");
    let project_dir = dir.join("demo");
    scaffold(
        Template::OneShot,
        &project_dir,
        &TemplateVars::for_project("demo").unwrap(),
    )
    .unwrap();
    let report = pack(&project_dir, &dir);
    let packed: Vec<&str> = report.index.sprites.keys().map(String::as_str).collect();
    assert!(packed.contains(&"sprites/player.png"), "{packed:?}");
    assert!(
        report
            .index
            .atlases
            .iter()
            .any(|a| a.category == "characters")
    );
    assert!(
        report
            .index
            .atlases
            .iter()
            .any(|a| a.category == "tilesets")
    );

    let folder = draws(&open_folder(&project_dir), 30);
    let pak = draws(&open_pak(&project_dir, &report.out).unwrap(), 30);
    assert_same_frames(&folder, &pak, &report.index);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn materials_see_the_same_uniforms_from_folder_and_pak() {
    let dir = fixture_root().join("materials-town");
    let out = temp_dir("materials");
    let report = pack(&dir, &out);
    let folder = draws(&open_folder(&dir), 10);
    let pak = draws(&open_pak(&dir, &report.out).unwrap(), 10);
    assert!(folder.iter().any(|d| d.material.is_some()));
    assert_same_frames(&folder, &pak, &report.index);
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn a_pak_for_another_engine_or_a_tampered_pak_does_not_start() {
    let dir = fixture_root().join("scenario-town");
    let out = temp_dir("mismatch");
    let report = pack(&dir, &out);

    // Repack with an engine requirement this runtime does not satisfy.
    let mut wrong_engine = report.index.clone();
    wrong_engine.engine = "99.0".into();
    let mut builder = pkrs_assets::PakBuilder::new(pkrs_assets::PakMeta {
        game_id: wrong_engine.game_id.clone(),
        game_version: wrong_engine.game_version.clone(),
        engine: wrong_engine.engine.clone(),
        api: wrong_engine.api,
        packed_by: wrong_engine.packed_by.clone(),
    });
    let source = pkrs_assets::PakVfs::open(&report.out).unwrap();
    for file in &report.index.files {
        let path = pkrs_assets::VfsPath::new(&file.path).unwrap();
        let bytes = pkrs_assets::Vfs::read(&source, &path).unwrap();
        if report.index.atlases.iter().any(|a| a.path == file.path) {
            let page = report
                .index
                .atlases
                .iter()
                .find(|a| a.path == file.path)
                .unwrap();
            builder.add_atlas_page(page.clone(), bytes).unwrap();
        } else {
            builder.add_file(path, bytes).unwrap();
        }
    }
    for (path, rect) in &report.index.sprites {
        builder
            .add_packed_image(&pkrs_assets::VfsPath::new(path).unwrap(), *rect)
            .unwrap();
    }
    let other = out.join("other-engine.pak");
    builder.write_to(&other).unwrap();
    let Err(err) = open_pak(&dir, &other) else {
        panic!("a pak for engine 99.0 must not open");
    };
    assert!(
        matches!(
            err,
            ProjectError::Pak(pkrs_assets::PakError::EngineMismatch { .. })
        ),
        "{err}"
    );

    // Flip one byte of the archive: the file table or a CRC no longer matches.
    let mut bytes = std::fs::read(&report.out).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0xff;
    let tampered = out.join("tampered.pak");
    std::fs::write(&tampered, bytes).unwrap();
    let opened = open_pak(&dir, &tampered);
    let read_all = opened.and_then(|project| {
        let index = project.vfs.pak_index().unwrap().clone();
        for file in &index.files {
            let path = pkrs_assets::VfsPath::new(&file.path)?;
            pkrs_assets::Vfs::read(&project.vfs, &path)?;
        }
        Ok(())
    });
    assert!(read_all.is_err(), "a corrupted archive must be detected");
    std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn the_runtime_binary_runs_a_scenario_from_the_pak() {
    let dir = fixture_root().join("scenario-town");
    let out = temp_dir("bin");
    let report = pack(&dir, &out);
    let run = Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--project"])
        .arg(&dir)
        .arg("--pak")
        .arg(&report.out)
        .args(["--script", "tests/talk_to_npc.lua"])
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(run.status.success(), "{stderr}");
    assert!(stderr.contains("PASS tests/talk_to_npc.lua"), "{stderr}");

    let headless = Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--headless", "--ticks", "60", "--pak"])
        .arg(&report.out)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&headless.stderr);
    assert!(headless.status.success(), "{stderr}");
    std::fs::remove_dir_all(&out).unwrap();
}
