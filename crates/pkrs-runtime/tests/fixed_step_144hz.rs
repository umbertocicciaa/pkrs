//! Integration: fixed-step loop with a scripted [`NullPlatform`](pkrs_platform::NullPlatform).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::{
    LoopExit, RuntimeLoopConfig, SimulationHost, StubSimulationHost, block_on_loop,
};

fn one_second_at_144hz_frames() -> Vec<NullFrame> {
    let step = 1.0 / 144.0;
    let mut frames: Vec<_> = std::iter::repeat_n(NullFrame::with_dt(step), 143).collect();
    frames.push(NullFrame::with_dt(1.0 - 143.0 * step));
    frames
}

struct CountingHost {
    ticks: u64,
}

impl SimulationHost for CountingHost {
    fn poll_input(&mut self, _: &Platform) {}

    fn begin_tick(&mut self) {}

    fn simulation_tick(&mut self, _: f64) {
        self.ticks = self.ticks.saturating_add(1);
    }

    fn render(&mut self, _: &Platform, _: f32) {}
}

#[test]
fn one_second_at_144hz_runs_exactly_sixty_ticks() {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(one_second_at_144hz_frames())),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );
    let mut host = CountingHost { ticks: 0 };
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: None,
            max_frames: Some(144),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::FrameLimit);
    assert_eq!(host.ticks, 60);
}

#[test]
fn stub_host_still_runs_under_tick_limit() {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(one_second_at_144hz_frames())),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );
    let mut host = StubSimulationHost;
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(60),
            max_frames: Some(144),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::TickLimit);
}
