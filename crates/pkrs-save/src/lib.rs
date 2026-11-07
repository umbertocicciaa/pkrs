//! Save system: save model, versioning, migrations and atomic writes.
//!
//! So far this crate holds the in-memory save model the overworld keeps during play:
//! [`WorldState`] (flags, variables, bag, money) with per-map [`MapState`] (§6.5), and the
//! dynamic [`Value`] type script variables are stored as. Serialisation, versioning and atomic
//! writes land with M6; see `docs/design.md` §4.3 and §12.

mod error;
mod value;
mod world;

pub use error::{Result, SaveError};
pub use value::Value;
pub use world::{MapState, ObjectState, WorldState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: SaveError = SaveError::UnsupportedVersion(7);
        assert_eq!(err.to_string(), "unsupported save version 7");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
