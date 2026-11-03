//! `NullPlatform`: a scripted, window-less backend for tests and `--headless` (§4.5, §15.2).

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use super::{FrameFuture, PlatformBackend};
use crate::key::Key;
use crate::{PlatformError, Result};

/// The state the [`NullPlatform`] reports for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct NullFrame {
    /// Reported frame time in seconds.
    pub dt: f64,
    /// Keys held during this frame.
    pub keys: Vec<Key>,
    /// Whether the (virtual) window has focus.
    pub focused: bool,
}

impl NullFrame {
    /// A focused frame of exactly one 60 Hz tick with no keys held.
    pub fn idle() -> Self {
        Self {
            dt: 1.0 / 60.0,
            keys: Vec::new(),
            focused: true,
        }
    }

    /// An idle frame lasting `dt` seconds.
    pub fn with_dt(dt: f64) -> Self {
        Self { dt, ..Self::idle() }
    }

    /// An idle frame with `keys` held.
    pub fn with_keys(keys: impl IntoIterator<Item = Key>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
            ..Self::idle()
        }
    }

    /// An idle frame with the given focus state.
    pub fn with_focus(focused: bool) -> Self {
        Self {
            focused,
            ..Self::idle()
        }
    }
}

impl Default for NullFrame {
    fn default() -> Self {
        Self::idle()
    }
}

/// Window-less [`PlatformBackend`] driven by a script of [`NullFrame`]s.
///
/// Before the first [`next_frame`](PlatformBackend::next_frame) it reports
/// [`NullFrame::idle`]. Each `next_frame` advances to the next scripted frame, or to
/// `NullFrame::idle` once the script is exhausted. Files live in memory.
#[derive(Clone, Debug)]
pub struct NullPlatform {
    script: VecDeque<NullFrame>,
    current: NullFrame,
    window_size: (u32, u32),
    fullscreen: bool,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Default for NullPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl NullPlatform {
    /// Creates a backend with an empty script and a 720×480 virtual window.
    pub fn new() -> Self {
        Self {
            script: VecDeque::new(),
            current: NullFrame::idle(),
            window_size: (720, 480),
            fullscreen: false,
            files: BTreeMap::new(),
        }
    }

    /// Appends frames to the script.
    pub fn with_frames(mut self, frames: impl IntoIterator<Item = NullFrame>) -> Self {
        self.script.extend(frames);
        self
    }

    /// Appends one frame to the script.
    pub fn push_frame(&mut self, frame: NullFrame) {
        self.script.push_back(frame);
    }

    /// Adds an in-memory file readable through [`PlatformBackend::read_file`].
    pub fn with_file(mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.into(), bytes.into());
        self
    }

    /// Whether the virtual window is fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }
}

impl PlatformBackend for NullPlatform {
    fn frame_time(&self) -> f64 {
        self.current.dt
    }

    fn next_frame(&mut self) -> FrameFuture<'_> {
        self.current = self.script.pop_front().unwrap_or_default();
        Box::pin(std::future::ready(()))
    }

    fn is_key_down(&self, key: Key) -> bool {
        self.current.keys.contains(&key)
    }

    fn keys_down(&self) -> Vec<Key> {
        let mut keys = self.current.keys.clone();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    fn has_focus(&self) -> bool {
        self.current.focused
    }

    fn window_size(&self) -> (u32, u32) {
        self.window_size
    }

    fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| PlatformError::FileNotFound(path.to_path_buf()))
    }

    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.files.insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }
}

/// Drives `fut` to completion on the current thread without an async runtime.
///
/// Intended for headless runs with [`NullPlatform`], whose frame futures are always
/// ready. It busy-polls (yielding the thread) if a future is pending.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = fut.as_mut().poll(&mut cx) {
            return output;
        }
        std::thread::yield_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_advances_per_frame_then_idles() {
        let mut backend = NullPlatform::new()
            .with_frames([NullFrame::with_dt(0.5), NullFrame::with_keys([Key::Z])]);
        assert_eq!(backend.frame_time(), 1.0 / 60.0);

        block_on(backend.next_frame());
        assert_eq!(backend.frame_time(), 0.5);
        assert!(!backend.is_key_down(Key::Z));

        block_on(backend.next_frame());
        assert!(backend.is_key_down(Key::Z));
        assert_eq!(backend.keys_down(), vec![Key::Z]);

        block_on(backend.next_frame());
        assert_eq!(backend.current, NullFrame::idle());
    }

    #[test]
    fn files_round_trip_in_memory() {
        let mut backend = NullPlatform::new().with_file("a.txt", b"hello".to_vec());
        assert_eq!(backend.read_file(Path::new("a.txt")).unwrap(), b"hello");
        backend.write_file(Path::new("b.bin"), &[1, 2]).unwrap();
        assert_eq!(backend.read_file(Path::new("b.bin")).unwrap(), vec![1, 2]);
        assert!(matches!(
            backend.read_file(Path::new("missing")),
            Err(PlatformError::FileNotFound(_))
        ));
    }

    #[test]
    fn window_mode_is_recorded() {
        let mut backend = NullPlatform::new();
        backend.set_window_size(320, 200);
        backend.set_fullscreen(true);
        assert_eq!(backend.window_size(), (320, 200));
        assert!(backend.is_fullscreen());
    }
}
