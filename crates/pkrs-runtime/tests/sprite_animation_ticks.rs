//! Integration: simulation ticks drive the M1 animation demo host draw list.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_render::Rect;
use pkrs_runtime::{AnimationDemoHost, LoopExit, RuntimeLoopConfig, block_on_loop};

#[test]
fn headless_ticks_select_expected_animation_frame_in_draw_list() {
    let frame_count = 8;
    let mut platform = Platform::new(
        Box::new(
            NullPlatform::new().with_frames(std::iter::repeat_n(NullFrame::idle(), frame_count)),
        ),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );
    let (mut host, log) = AnimationDemoHost::headless().expect("demo host");
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: None,
            max_frames: Some(frame_count as u64),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::FrameLimit);

    let xs: Vec<f32> = log.draws().iter().map(|d| d.src.x).collect();
    assert_eq!(xs, [0.0, 0.0, 16.0, 16.0, 16.0, 32.0, 32.0, 32.0]);
}

#[test]
fn tick_timing_matches_at_different_refresh_rates() {
    fn one_second_at_30hz() -> Vec<NullFrame> {
        std::iter::repeat_n(NullFrame::with_dt(1.0 / 30.0), 30).collect()
    }

    fn one_second_at_144hz() -> Vec<NullFrame> {
        let step = 1.0 / 144.0;
        let mut frames: Vec<_> = std::iter::repeat_n(NullFrame::with_dt(step), 143).collect();
        frames.push(NullFrame::with_dt(1.0 - 143.0 * step));
        frames
    }

    let run_sixty_ticks = |frames: Vec<NullFrame>| -> Rect {
        let mut platform = Platform::new(
            Box::new(NullPlatform::new().with_frames(frames)),
            Box::new(FixedPaths::under("test")),
            PlatformConfig::default(),
        );
        let (mut host, log) = AnimationDemoHost::headless().expect("demo host");
        let exit = block_on_loop(
            &mut platform,
            &mut host,
            &RuntimeLoopConfig {
                max_ticks: Some(60),
                max_frames: None,
                ..RuntimeLoopConfig::default()
            },
        );
        assert_eq!(exit, LoopExit::TickLimit);
        log.draws().last().expect("at least one draw").src
    };

    let at_30 = run_sixty_ticks(one_second_at_30hz());
    let at_144 = run_sixty_ticks(one_second_at_144hz());
    assert_eq!(at_30, at_144);
}
