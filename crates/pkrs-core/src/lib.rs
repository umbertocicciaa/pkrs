//! Shared engine types: ids, `Direction`, `GridPos`, `Ctx` traits, events, errors and the `GameRng` port.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{CoreError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: CoreError = CoreError::InvalidId("bad id".into());
        assert_eq!(err.to_string(), "invalid id `bad id`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
