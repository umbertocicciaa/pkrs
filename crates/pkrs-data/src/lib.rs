//! Data-driven content: RON schemas, loading, validation and cross-reference checks.
//!
//! See `docs/design.md` §7.3 for battle-related schemas (full content pipeline in M6).

mod battle_pack;
mod error;
mod move_def;
mod species;
mod type_chart;

pub use battle_pack::BattlePack;
pub use error::{DataError, Result};
pub use move_def::{MoveCategory, MoveDef, MoveEffect, StatId};
pub use species::SpeciesDef;
pub use type_chart::TypeChart;

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
