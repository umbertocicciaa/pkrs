//! The `Platform` facade: the game-facing service over a [`PlatformBackend`] (§4.5).

use std::path::Path;

use crate::Result;
use crate::backend::PlatformBackend;
use crate::key::Key;
use crate::paths::UserPaths;
use crate::window::DisplaySettings;

/// Longest frame handed to the fixed-step accumulator, in seconds (§3.1 `MAX_FRAME`).
pub const DEFAULT_MAX_FRAME_TIME: f64 = 0.25;

/// Engine policy applied by the [`Platform`] facade on top of the backend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlatformConfig {
    /// Clamp for [`Platform::frame_time`], avoiding the "spiral of death" after a stall.
    pub max_frame_time: f64,
    /// When `true`, [`Platform::frame_time`] reports `0.0` while the window is unfocused,
    /// so the simulation pauses (§3.2).
    pub pause_on_focus_lost: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            max_frame_time: DEFAULT_MAX_FRAME_TIME,
            pause_on_focus_lost: true,
        }
    }
}

/// Notifications raised by the facade, drained with [`Platform::drain_events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformEvent {
    /// The window lost input focus (the runtime pauses and ducks audio).
    FocusLost,
    /// The window regained input focus.
    FocusGained,
}

/// Game-facing platform service: frame timing, raw keys, focus, window mode, files, paths.
///
/// It holds any [`PlatformBackend`] (macroquad or null) behind a trait object, so the
/// same code runs windowed and headless.
pub struct Platform {
    backend: Box<dyn PlatformBackend>,
    paths: Box<dyn UserPaths>,
    config: PlatformConfig,
    focused: bool,
    fullscreen: bool,
    frame: u64,
    events: Vec<PlatformEvent>,
}

impl Platform {
    /// Wraps `backend` and `paths` with the given policy.
    pub fn new(
        backend: Box<dyn PlatformBackend>,
        paths: Box<dyn UserPaths>,
        config: PlatformConfig,
    ) -> Self {
        let focused = backend.has_focus();
        Self {
            backend,
            paths,
            config,
            focused,
            fullscreen: false,
            frame: 0,
            events: Vec::new(),
        }
    }

    /// The policy this facade applies.
    pub fn config(&self) -> &PlatformConfig {
        &self.config
    }

    /// Duration of the last frame for the fixed-step accumulator (§3.1), in seconds.
    ///
    /// Clamped to `[0, max_frame_time]` (non-finite values become `0`), and `0` while
    /// paused by focus loss.
    pub fn frame_time(&self) -> f64 {
        if self.is_paused() {
            return 0.0;
        }
        let raw = self.backend.frame_time();
        if raw.is_finite() {
            raw.clamp(0.0, self.config.max_frame_time.max(0.0))
        } else {
            0.0
        }
    }

    /// The backend's unclamped frame time, for debug overlays (§15.1).
    pub fn raw_frame_time(&self) -> f64 {
        self.backend.frame_time()
    }

    /// Presents the frame and waits for the next one, then refreshes focus state.
    pub async fn next_frame(&mut self) {
        self.backend.next_frame().await;
        self.frame = self.frame.saturating_add(1);
        self.sync_focus();
    }

    /// Number of frames presented through [`next_frame`](Self::next_frame).
    pub fn frame_count(&self) -> u64 {
        self.frame
    }

    /// Whether the window has input focus (as of the last frame).
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the simulation should be paused (unfocused and `pause_on_focus_lost`).
    pub fn is_paused(&self) -> bool {
        self.config.pause_on_focus_lost && !self.focused
    }

    /// Takes the notifications raised since the last call, in order.
    pub fn drain_events(&mut self) -> Vec<PlatformEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether `key` is currently held.
    pub fn is_key_down(&self, key: Key) -> bool {
        self.backend.is_key_down(key)
    }

    /// Every key currently held, in [`Key`] order.
    pub fn keys_down(&self) -> Vec<Key> {
        self.backend.keys_down()
    }

    /// Current window size in pixels.
    pub fn window_size(&self) -> (u32, u32) {
        self.backend.window_size()
    }

    /// Whether the window was last switched to fullscreen through this facade.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Switches between fullscreen and windowed mode.
    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
        self.backend.set_fullscreen(fullscreen);
    }

    /// Applies `pkrs.toml [display]` (or the user's override): windowed size and mode.
    pub fn apply_display(&mut self, display: &DisplaySettings) {
        let (width, height) = display.window_size();
        self.backend.set_window_size(width, height);
        self.set_fullscreen(display.fullscreen);
    }

    /// Per-user directories for saves, config and logs (§14.3).
    pub fn paths(&self) -> &dyn UserPaths {
        self.paths.as_ref()
    }

    /// Reads a whole file through the backend.
    ///
    /// For user files on real filesystem paths
    /// (saves, config, logs under [`UserPaths`](crate::UserPaths)). Not the assets VFS: game
    /// content goes through `pkrs-assets` (§13).
    pub fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.backend.read_file(path)
    }

    /// Writes a whole file through the backend (no atomic replace; `pkrs-save` adds that).
    ///
    /// For user files on real filesystem paths
    /// (saves, config, logs under [`UserPaths`](crate::UserPaths)). Not the assets VFS: game
    /// content goes through `pkrs-assets` (§13).
    pub fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.backend.write_file(path, bytes)
    }

    fn sync_focus(&mut self) {
        let focused = self.backend.has_focus();
        if focused != self.focused {
            self.focused = focused;
            self.events.push(if focused {
                PlatformEvent::FocusGained
            } else {
                PlatformEvent::FocusLost
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{NullFrame, NullPlatform, block_on};
    use crate::paths::FixedPaths;

    fn platform(frames: impl IntoIterator<Item = NullFrame>, config: PlatformConfig) -> Platform {
        Platform::new(
            Box::new(NullPlatform::new().with_frames(frames)),
            Box::new(FixedPaths::under("root")),
            config,
        )
    }

    #[test]
    fn frame_time_is_clamped_to_max_frame() {
        let mut p = platform(
            [
                NullFrame::with_dt(1.0),
                NullFrame::with_dt(0.1),
                NullFrame::with_dt(-1.0),
                NullFrame::with_dt(f64::NAN),
            ],
            PlatformConfig::default(),
        );
        block_on(p.next_frame());
        assert_eq!(p.frame_time(), DEFAULT_MAX_FRAME_TIME);
        assert_eq!(p.raw_frame_time(), 1.0);
        block_on(p.next_frame());
        assert_eq!(p.frame_time(), 0.1);
        block_on(p.next_frame());
        assert_eq!(p.frame_time(), 0.0);
        block_on(p.next_frame());
        assert_eq!(p.frame_time(), 0.0);
    }

    #[test]
    fn custom_clamp_is_respected() {
        let config = PlatformConfig {
            max_frame_time: 0.05,
            ..PlatformConfig::default()
        };
        let mut p = platform([NullFrame::with_dt(0.2)], config);
        block_on(p.next_frame());
        assert_eq!(p.frame_time(), 0.05);
    }

    #[test]
    fn focus_changes_raise_events_once() {
        let mut p = platform(
            [
                NullFrame::with_focus(false),
                NullFrame::with_focus(false),
                NullFrame::with_focus(true),
            ],
            PlatformConfig::default(),
        );
        assert!(p.is_focused());
        block_on(p.next_frame());
        block_on(p.next_frame());
        assert!(!p.is_focused());
        assert_eq!(p.drain_events(), vec![PlatformEvent::FocusLost]);
        assert!(p.drain_events().is_empty());
        block_on(p.next_frame());
        assert_eq!(p.drain_events(), vec![PlatformEvent::FocusGained]);
    }

    #[test]
    fn focus_loss_pauses_only_when_configured() {
        let unfocused = NullFrame {
            dt: 0.1,
            ..NullFrame::with_focus(false)
        };
        let mut paused = platform([unfocused.clone()], PlatformConfig::default());
        block_on(paused.next_frame());
        assert!(paused.is_paused());
        assert_eq!(paused.frame_time(), 0.0);

        let config = PlatformConfig {
            pause_on_focus_lost: false,
            ..PlatformConfig::default()
        };
        let mut running = platform([unfocused], config);
        block_on(running.next_frame());
        assert!(!running.is_paused());
        assert_eq!(running.frame_time(), 0.1);
    }

    #[test]
    fn keys_pass_through() {
        let mut p = platform(
            [NullFrame::with_keys([Key::Right, Key::Z])],
            PlatformConfig::default(),
        );
        block_on(p.next_frame());
        assert!(p.is_key_down(Key::Z));
        assert!(!p.is_key_down(Key::X));
        assert_eq!(p.keys_down(), vec![Key::Z, Key::Right]);
    }

    #[test]
    fn apply_display_sets_size_and_mode() {
        let mut p = platform([], PlatformConfig::default());
        p.apply_display(&DisplaySettings {
            width: 240,
            height: 160,
            scale: 2,
            fullscreen: true,
        });
        assert_eq!(p.window_size(), (480, 320));
        assert!(p.is_fullscreen());
    }

    #[test]
    fn paths_and_files_delegate() {
        let mut p = platform([], PlatformConfig::default());
        assert_eq!(p.paths().data_dir(), Path::new("root").join("data"));
        p.write_file(Path::new("x"), b"1").unwrap();
        assert_eq!(p.read_file(Path::new("x")).unwrap(), b"1");
    }
}
