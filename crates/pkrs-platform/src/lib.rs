//! Platform facade and ports: window, main loop, keyboard, file I/O and user paths.
//!
//! Layout (Ports & Adapters, `docs/design.md` §4.5):
//!
//! - **Ports** (engine-owned traits): [`PlatformBackend`], [`UserPaths`].
//! - **Facade**: [`Platform`], which adds engine policy (frame time clamping, pause on
//!   focus loss, focus events) on top of any backend.
//! - **Adapters**: [`MacroquadPlatform`] (feature `macroquad`), [`DirectoriesPaths`]
//!   (feature `directories`), and the vendor-free [`NullPlatform`] / [`FixedPaths`]
//!   for tests and headless runs.
//!
//! No vendor type appears in any public signature. Gamepad support (`GilrsGamepad`)
//! and action mapping (the `Input` facade) come later.

mod backend;
mod error;
mod key;
mod paths;
mod platform;
mod window;

#[cfg(feature = "directories")]
pub use backend::DirectoriesPaths;
#[cfg(feature = "macroquad")]
pub use backend::MacroquadPlatform;
pub use backend::{FrameFuture, NullFrame, NullPlatform, PlatformBackend, block_on};
pub use error::{PlatformError, Result};
pub use key::Key;
pub use paths::{FixedPaths, UserPaths};
pub use platform::{DEFAULT_MAX_FRAME_TIME, Platform, PlatformConfig, PlatformEvent};
pub use window::{DisplaySettings, WindowConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: PlatformError = PlatformError::UserPathsUnavailable;
        assert_eq!(err.to_string(), "user directories are unavailable");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }

    #[test]
    fn file_not_found_mentions_path() {
        let err = PlatformError::FileNotFound("saves/slot1.ron".into());
        assert!(err.to_string().contains("slot1.ron"));
    }
}
