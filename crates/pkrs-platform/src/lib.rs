//! Platform facade and ports: window, main loop, keyboard, gamepad, file I/O and user paths.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{PlatformError, Result};

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
}
