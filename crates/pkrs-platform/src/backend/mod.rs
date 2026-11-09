//! The `PlatformBackend` port and its adapters (§4.5).
//!
//! Each vendor adapter lives in its own module behind a cargo feature and is the
//! only place that vendor's types appear.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use crate::Result;
use crate::key::Key;

#[cfg(feature = "directories")]
mod directories;
#[cfg(feature = "gilrs")]
mod gilrs;
#[cfg(feature = "macroquad")]
mod macroquad;
mod null;

#[cfg(feature = "directories")]
pub use self::directories::DirectoriesPaths;
#[cfg(feature = "gilrs")]
pub use self::gilrs::GilrsGamepad;
#[cfg(feature = "macroquad")]
pub use self::macroquad::MacroquadPlatform;
pub use self::null::{NullFrame, NullPlatform, block_on};

/// Future returned by [`PlatformBackend::next_frame`]; resolves when the next frame may begin.
pub type FrameFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Engine-owned port over the windowing / OS layer.
///
/// It is the minimum the engine needs from a backend: frame timing, presenting a
/// frame, raw keyboard state, focus, window mode and file I/O. Policy (frame time
/// clamping, pause on focus loss, focus events) lives in the [`Platform`](crate::Platform)
/// facade, not here.
pub trait PlatformBackend {
    /// Wall-clock duration of the last frame, in seconds, unclamped.
    fn frame_time(&self) -> f64;

    /// Presents the current frame and waits until the next one may begin.
    ///
    /// Raw input and focus state are refreshed when the future resolves.
    fn next_frame(&mut self) -> FrameFuture<'_>;

    /// Whether `key` is currently held.
    fn is_key_down(&self, key: Key) -> bool;

    /// Every key currently held, in [`Key`] order.
    fn keys_down(&self) -> Vec<Key>;

    /// Whether the game window currently has input focus.
    fn has_focus(&self) -> bool;

    /// Current window size in pixels.
    fn window_size(&self) -> (u32, u32);

    /// Requests a new windowed size in pixels (ignored while fullscreen by most backends).
    fn set_window_size(&mut self, width: u32, height: u32);

    /// Switches between fullscreen and windowed mode.
    fn set_fullscreen(&mut self, fullscreen: bool);

    /// Reads a whole file from a real filesystem path.
    ///
    /// For user files on real filesystem paths
    /// (saves, config, logs under [`UserPaths`](crate::UserPaths)). Not the assets VFS: game
    /// content goes through `pkrs-assets` (§13).
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Writes a whole file to a real filesystem path, replacing it if it exists.
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<()>;
}
