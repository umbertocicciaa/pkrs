//! World model: maps, collision, grid movement, warps, connections, NPCs and the `MapImporter` port.
//!
//! This crate is currently a skeleton; see `docs/design.md` §4.3 for its role
//! and dependency boundaries.

mod error;

pub use error::{Result, WorldError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: WorldError = WorldError::MapImport("route_1".into());
        assert_eq!(err.to_string(), "failed to import map `route_1`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
