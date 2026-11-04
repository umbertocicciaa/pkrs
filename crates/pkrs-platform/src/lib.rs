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
//! **File I/O** here ([`Platform::read_file`] / [`Platform::write_file`]) is for per-user
//! files on real filesystem paths (saves, config, logs under [`UserPaths`]). It is **not**
//! the assets VFS: game content is read through `pkrs-assets` (`Vfs`: loose folder or
//! `.pak`, §13).
//!
//! No vendor type appears in any public signature. Gamepad support (`GilrsGamepad`)
//! is planned; keyboard actions are handled by the [`Input`] facade (§10).

mod action;
mod backend;
mod error;
mod input;
mod key;
mod paths;
mod platform;
mod window;

pub use action::{Action, ActionRegistry, BuiltinAction, CustomActionId, resolve_action};
#[cfg(feature = "directories")]
pub use backend::DirectoriesPaths;
#[cfg(feature = "macroquad")]
pub use backend::MacroquadPlatform;
pub use backend::{FrameFuture, NullFrame, NullPlatform, PlatformBackend, block_on};
pub use error::{InputError, InputResult, PlatformError, Result};
pub use input::{Input, InputFacadeConfig, MenuRepeatConfig, MovementBufferConfig};
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
