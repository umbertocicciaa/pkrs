//! Acceptance (§5.6): the `materials-town` fixture shows a day/night tint and animated water.
//! A headless run with `NullRenderer` records, for every tick, the material bound to each draw
//! and its uniform values: the water tiles step through their frames with `time_ticks`, the
//! ground's tint follows the `day_tint` global that `scripts/main.lua` cycles with
//! `render.set_global_uniform`, and the ghost NPC runs its project GLSL shader.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_core::Pcg64Rng;
use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{
    DrawLog, Layer, MaterialHandle, NullRenderer, RecordedDraw, Renderer, RendererConfig,
    UniformValue,
};
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{SimulationHost, TICK_DT};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/projects/materials-town")
}

fn platform() -> Platform {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([NullFrame::idle()])),
        Box::new(FixedPaths::under("materials")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    platform
}

fn host() -> (ProjectHost, DrawLog) {
    let project = Project::open(&ProjectOptions {
        dir: fixture(),
        ..ProjectOptions::default()
    })
    .unwrap();
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(66))).unwrap();
    assert!(report.failed.is_empty());
    let null = NullRenderer::new();
    let log = null.draw_log();
    let renderer = Renderer::new(Box::new(null), project.display(), RendererConfig::default());
    (
        ProjectHost::with_scripts(&project, renderer, scripts).unwrap(),
        log,
    )
}

fn handle(log: &DrawLog, name: &str) -> MaterialHandle {
    log.materials()
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("material `{name}` was not created"))
        .handle
}

fn uniform(draw: &RecordedDraw, name: &str) -> Option<UniformValue> {
    draw.material.as_ref().and_then(|b| b.uniform(name))
}

const DAY: UniformValue = UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]);
const DUSK: UniformValue = UniformValue::Vec4([1.0, 0.8, 0.6, 1.0]);
const NIGHT: UniformValue = UniformValue::Vec4([0.35, 0.4, 0.7, 1.0]);

#[test]
fn day_night_tint_and_animated_water_over_ticks() {
    let (mut host, log) = host();
    let platform = platform();
    let names: Vec<String> = log.materials().iter().map(|m| m.name.clone()).collect();
    assert_eq!(names, ["water", "day_night", "ghost"], "compiled once each");
    let (water, day_night, ghost) = (
        handle(&log, "water"),
        handle(&log, "day_night"),
        handle(&log, "ghost"),
    );

    let mut tint_changes: Vec<(u64, UniformValue)> = Vec::new();
    for _ in 0..100 {
        host.poll_input(&platform);
        host.begin_tick();
        host.simulation_tick(TICK_DT);
        host.render(&platform, 0.0);
        let ticks = host.ticks();

        let draws = log.draws();
        let ground: Vec<&RecordedDraw> =
            draws.iter().filter(|d| d.layer == Layer::Ground).collect();
        assert_eq!(ground.len(), 24, "6x4 ground tiles");
        let (water_tiles, grass): (Vec<_>, Vec<_>) = ground.iter().partition(|d| d.src.y == 16.0);
        assert_eq!(water_tiles.len(), 4);

        // Animated water: 4 frames of 15 ticks, 16 px apart, tinted like the ground.
        let expected_frame = ((ticks / 15) % 4) as f32;
        let tint = uniform(grass[0], "tint").unwrap();
        for tile in &water_tiles {
            let bind = tile.material.as_ref().unwrap();
            assert_eq!(bind.material, water);
            assert_eq!(
                bind.uniform("frame"),
                Some(UniformValue::Float(expected_frame))
            );
            assert_eq!(
                bind.uniform("frame_offset"),
                Some(UniformValue::Vec2([16.0, 0.0]))
            );
            assert_eq!(
                bind.uniform("texture_size"),
                Some(UniformValue::Vec2([64.0, 32.0]))
            );
            assert_eq!(bind.uniform("tint"), Some(tint));
        }
        // Day/night: every other ground tile uses the layer's tint material.
        for tile in &grass {
            let bind = tile.material.as_ref().unwrap();
            assert_eq!(bind.material, day_night);
            assert_eq!(bind.uniform("tint"), Some(tint));
            assert_eq!(bind.uniform("strength"), Some(UniformValue::Float(1.0)));
        }
        if tint_changes.last().map(|(_, t)| *t) != Some(tint) {
            tint_changes.push((ticks, tint));
        }

        // The ghost NPC runs the project shader with the engine's `time_ticks`.
        let npc = draws
            .iter()
            .filter(|d| d.layer == Layer::Entities)
            .find(|d| d.material.is_some())
            .expect("ghost NPC drawn with its material");
        let bind = npc.material.as_ref().unwrap();
        assert_eq!(bind.material, ghost);
        assert_eq!(
            bind.uniforms,
            [
                ("time_ticks".to_owned(), UniformValue::Float(ticks as f32)),
                ("opacity".to_owned(), UniformValue::Float(0.6)),
            ]
        );
        drop(draws);
        log.clear();
    }

    let phases: Vec<UniformValue> = tint_changes.iter().map(|(_, t)| *t).collect();
    assert_eq!(phases, [DAY, DUSK, NIGHT, DAY], "{tint_changes:?}");
    // Day from the first frame; each phase lasts `PHASE_TICKS` = 30 ticks.
    let at: Vec<u64> = tint_changes.iter().map(|(t, _)| *t).collect();
    assert_eq!(at, [1, 30, 60, 90], "{tint_changes:?}");
}

#[test]
fn rust_hooks_set_global_uniforms_too() {
    let (mut host, log) = host();
    let platform = platform();
    host.set_global_uniform("day_tint", UniformValue::Vec4([0.0, 0.0, 0.0, 1.0]));
    host.render(&platform, 0.0);
    let draws = log.draws();
    let grass = draws
        .iter()
        .find(|d| d.layer == Layer::Ground && d.src.y == 0.0)
        .unwrap();
    assert_eq!(
        uniform(grass, "tint"),
        Some(UniformValue::Vec4([0.0, 0.0, 0.0, 1.0]))
    );
}
