//! Assets facade: virtual file system (loose folder or `.pak`), handles and caching.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{AssetsError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: AssetsError = AssetsError::NotFound("gfx/hero.png".into());
        assert_eq!(err.to_string(), "asset not found: gfx/hero.png");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
