//! Loads the shared fixture projects in `tests/fixtures/projects/` (workspace root) from disk
//! with `ProjectManifest::load`, the same entry point the runtime and CLI use (§2.2).

use std::path::{Path, PathBuf};

use pkrs_core::manifest::{DisplayConfig, ManifestError};
use pkrs_core::{Direction, GridPos, ProjectManifest, Version};

/// Engine version and API levels these fixtures are checked against.
const ENGINE: Version = Version::new(0, 0, 0);

fn project(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/projects")
        .join(name)
}

fn load(name: &str) -> Result<ProjectManifest, ManifestError> {
    ProjectManifest::load(&project(name))
}

#[test]
fn minimal_project_loads_with_defaults() {
    let m = load("minimal").expect("minimal fixture is valid");
    assert_eq!(m.game.id, "com.example.minimal");
    assert_eq!(m.display, DisplayConfig::default());
    assert_eq!(m.start.position, GridPos::new(3, 4));
    assert_eq!(m.start.facing, Direction::Down);
    assert!(m.features.battles);
    assert_eq!(m.locale.available, ["en"]);
    m.check_engine(&ENGINE, 1..=1)
        .expect("compatible with engine 0.0.x");
}

#[test]
fn full_project_loads_every_table() {
    let m = load("full").expect("full fixture is valid");
    assert_eq!(m.game.name, "Lighthouse Tale");
    assert_eq!(m.game.authors, ["Jane Doe"]);
    assert!(m.engine.accepts(&Version::new(0, 3, 5)));
    assert!(!m.engine.accepts(&ENGINE));
    assert_eq!(m.start.script.as_deref(), Some("scripts/main.lua"));
    assert_eq!(m.plugins.enabled, ["resonance", "day-night"]);
    assert_eq!(m.locale.available, ["en", "it"]);
    assert_eq!(m.input.bindings.len(), 9);
    assert_eq!(m.input.bindings["confirm"], ["z", "enter"]);

    let err = m
        .check_engine(&ENGINE, 1..=1)
        .expect_err("needs engine 0.3");
    assert_eq!(err.issues()[0].path, "engine.version");
}

#[test]
fn story_only_project_disables_battles() {
    let dir = project("story-only");
    let m = ProjectManifest::load(&dir).expect("story fixture is valid");
    assert!(!m.features.battles && !m.features.party);
    assert_eq!(m.start.facing, Direction::Up);
    assert_eq!(m.locale.default, "it");
    let script = m.start.script.as_deref().expect("has a start script");
    assert!(dir.join(script).is_file(), "start script exists on disk");
}

#[test]
fn invalid_project_reports_all_field_paths() {
    let err = load("invalid").expect_err("invalid fixture");
    let paths: Vec<_> = err.issues().iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "game.id",
            "engine.version",
            "engine.api",
            "display.scale",
            "start.map",
            "start.facing"
        ]
    );
}

#[test]
fn broken_toml_reports_a_location() {
    let err = load("broken-toml").expect_err("syntax error");
    assert!(matches!(err, ManifestError::Parse(_)), "{err:?}");
    let location = err.issues()[0]
        .location
        .expect("syntax errors have a location");
    assert_eq!(location.line, 2);
}

#[test]
fn missing_project_is_an_io_error() {
    let err = load("does-not-exist").expect_err("no such project");
    assert!(matches!(err, ManifestError::Io { .. }), "{err:?}");
}
