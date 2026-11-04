//! Main loop driven by the [`Platform`](pkrs_platform::Platform) facade (§3.1–§3.3).

use pkrs_platform::Platform;

use crate::fixed_step::{FixedStepClock, TICK_DT};
use crate::tick::run_tick;

/// Why the loop stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopExit {
    /// Reached [`RuntimeLoopConfig::max_ticks`].
    TickLimit,
    /// Reached [`RuntimeLoopConfig::max_frames`].
    FrameLimit,
}

/// Policy for the outer loop (§3.1–§3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLoopConfig {
    /// Stop after this many simulation ticks (`None` = unlimited).
    pub max_ticks: Option<u64>,
    /// Stop after this many presented frames (`None` = unlimited).
    pub max_frames: Option<u64>,
    /// When `false`, the window backend should disable VSync (debug only, §3.2).
    pub vsync: bool,
}

impl Default for RuntimeLoopConfig {
    fn default() -> Self {
        Self {
            max_ticks: None,
            max_frames: None,
            vsync: true,
        }
    }
}

/// Host for one running game: input polling, ticks, render.
pub trait SimulationHost {
    /// Poll keyboard / gamepad once per rendered frame (§3.1).
    ///
    /// At high refresh rates many frames run zero simulation ticks, so implementations
    /// must latch edge-triggered input (e.g. key-down) until the next [`begin_tick`](Self::begin_tick).
    fn poll_input(&mut self, platform: &Platform);
    /// Edge detection reset at the start of each simulation tick.
    fn begin_tick(&mut self);
    /// One 60 Hz tick through §3.3; the host runs [`run_tick`](crate::tick::run_tick) with its own hooks.
    fn simulation_tick(&mut self, dt: f64);
    /// Draw with interpolation factor `alpha` (§3.1).
    fn render(&mut self, platform: &Platform, alpha: f32);
}

/// Default host that runs stub §3.3 hooks only.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubSimulationHost;

impl SimulationHost for StubSimulationHost {
    fn poll_input(&mut self, _platform: &Platform) {}

    fn begin_tick(&mut self) {}

    fn simulation_tick(&mut self, dt: f64) {
        run_tick(&mut crate::tick::NullTickHooks, dt);
    }

    fn render(&mut self, _platform: &Platform, _alpha: f32) {}
}

/// Runs the fixed-step loop until a limit is hit or the future is dropped.
pub async fn run_loop(
    platform: &mut Platform,
    host: &mut impl SimulationHost,
    config: &RuntimeLoopConfig,
) -> LoopExit {
    let mut clock = FixedStepClock::new();
    let mut sim_ticks: u64 = 0;

    loop {
        host.poll_input(platform);
        let frame_time = platform.frame_time();
        let ticks_this_frame = clock.advance(frame_time);
        for _ in 0..ticks_this_frame {
            host.begin_tick();
            host.simulation_tick(TICK_DT);
            sim_ticks = sim_ticks.saturating_add(1);
            if config.max_ticks.is_some_and(|limit| sim_ticks >= limit) {
                return LoopExit::TickLimit;
            }
        }

        let alpha = clock.alpha();
        host.render(platform, alpha);
        platform.next_frame().await;

        if config
            .max_frames
            .is_some_and(|limit| platform.frame_count() >= limit)
        {
            return LoopExit::FrameLimit;
        }
    }
}

/// Drives [`run_loop`] on the current thread (for headless [`NullPlatform`](pkrs_platform::NullPlatform)).
pub fn block_on_loop(
    platform: &mut Platform,
    host: &mut impl SimulationHost,
    config: &RuntimeLoopConfig,
) -> LoopExit {
    pkrs_platform::block_on(run_loop(platform, host, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};

    fn platform_with_frames(frames: impl IntoIterator<Item = NullFrame>) -> Platform {
        Platform::new(
            Box::new(NullPlatform::new().with_frames(frames)),
            Box::new(FixedPaths::under("test")),
            PlatformConfig::default(),
        )
    }

    #[test]
    fn tick_limit_stops_loop() {
        let mut platform = platform_with_frames(std::iter::repeat_n(NullFrame::idle(), 100));
        let mut host = StubSimulationHost;
        let exit = block_on_loop(
            &mut platform,
            &mut host,
            &RuntimeLoopConfig {
                max_ticks: Some(10),
                ..RuntimeLoopConfig::default()
            },
        );
        assert_eq!(exit, LoopExit::TickLimit);
    }
}
