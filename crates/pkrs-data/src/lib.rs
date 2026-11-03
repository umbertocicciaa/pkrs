//! Data-driven content: RON schemas, loading, validation and cross-reference checks.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{DataError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: DataError = DataError::UnknownReference {
            kind: "move".into(),
            id: "ember".into(),
        };
        assert_eq!(err.to_string(), "unknown move `ember`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
