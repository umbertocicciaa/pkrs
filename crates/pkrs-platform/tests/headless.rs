//! Headless integration: drive the `Platform` facade with `NullPlatform` through the
//! §3.1 fixed-step loop for N frames.

// Test-only helpers outside `#[test]` fns are not covered by clippy.toml's test exemption.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{
    FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformBackend, PlatformConfig,
    PlatformEvent, block_on,
};

const DT: f64 = 1.0 / 60.0;

/// What the runtime loop observed; written against the facade only, so the same code
/// runs on `MacroquadPlatform`.
#[derive(Default)]
struct Observed {
    ticks: u64,
    z_down_frames: u64,
    events: Vec<PlatformEvent>,
}

async fn run_frames(platform: &mut Platform, frames: u64) -> Observed {
    let mut observed = Observed::default();
    let mut acc = 0.0;
    for _ in 0..frames {
        acc += platform.frame_time();
        while acc >= DT - 1e-9 {
            observed.ticks += 1;
            acc -= DT;
        }
        if platform.is_key_down(Key::Z) {
            observed.z_down_frames += 1;
        }
        platform.next_frame().await;
        observed.events.extend(platform.drain_events());
    }
    observed
}

fn headless(backend: NullPlatform) -> Platform {
    Platform::new(
        Box::new(backend),
        Box::new(FixedPaths::under(
            std::env::temp_dir().join("pkrs-headless"),
        )),
        PlatformConfig::default(),
    )
}

#[test]
fn runs_n_idle_frames_at_one_tick_each() {
    let mut platform = headless(NullPlatform::new());
    let observed = block_on(run_frames(&mut platform, 600));
    assert_eq!(platform.frame_count(), 600);
    assert_eq!(observed.ticks, 600);
    assert!(observed.events.is_empty());
}

#[test]
fn stall_is_clamped_and_focus_loss_pauses() {
    let mut frames = vec![NullFrame::with_dt(5.0)]; // a 5 s stall
    frames.extend((0..10).map(|_| NullFrame::with_focus(false)));
    frames.push(NullFrame::with_keys([Key::Z]));
    let mut platform = headless(NullPlatform::new().with_frames(frames));

    let observed = block_on(run_frames(&mut platform, 20));

    // Frame 0 is the initial idle frame (1 tick). The 5 s stall is clamped to 0.25 s
    // (15 ticks). The 10 unfocused frames add nothing, and the remaining 8 idle frames
    // (the Z frame included) add one tick each.
    assert_eq!(observed.ticks, 1 + 15 + 8);
    assert_eq!(observed.z_down_frames, 1);
    assert_eq!(
        observed.events,
        vec![PlatformEvent::FocusLost, PlatformEvent::FocusGained]
    );
}

/// The facade takes any backend as `Box<dyn PlatformBackend>`: swapping adapters is a
/// one-line change at construction, with no change to code that uses `Platform`.
#[cfg(feature = "macroquad")]
#[allow(dead_code)]
fn macroquad_is_a_drop_in_backend(backend: pkrs_platform::MacroquadPlatform) -> Platform {
    let boxed: Box<dyn PlatformBackend> = Box::new(backend);
    Platform::new(
        boxed,
        Box::new(FixedPaths::under(".")),
        PlatformConfig::default(),
    )
}

#[test]
fn null_backend_is_a_drop_in_backend() {
    let boxed: Box<dyn PlatformBackend> = Box::new(NullPlatform::new());
    let platform = Platform::new(
        boxed,
        Box::new(FixedPaths::under(".")),
        PlatformConfig::default(),
    );
    assert!(platform.is_focused());
}
