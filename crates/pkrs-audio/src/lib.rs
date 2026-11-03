//! Audio facade and `AudioBackend` port: BGM, SFX, crossfades and ducking.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{AudioError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: AudioError = AudioError::Backend("no output device".into());
        assert_eq!(err.to_string(), "audio backend error: no output device");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
