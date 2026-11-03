//! Pure, deterministic battle logic. This crate must never depend on rendering, scripting or I/O.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{BattleError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: BattleError = BattleError::InvalidAction("no PP left".into());
        assert_eq!(err.to_string(), "invalid action: no PP left");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
