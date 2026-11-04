//! Shared engine types: ids, `Direction`, `GridPos`, `Ctx` traits, events, errors, the `GameRng`
//! port, semantic versions and the `pkrs.toml` project manifest.
//!
//! See `docs/design.md` §4.2–§4.3 and §4.5 for crate boundaries.

mod ctx;
mod direction;
mod error;
mod events;
mod grid;
mod id;
pub mod manifest;
mod rng;
mod version;

pub use ctx::{EventSink, RngAccess};
pub use direction::Direction;
pub use error::{CoreError, Result};
pub use events::{EngineEvent, EventPayload, EventQueue};
pub use grid::GridPos;
pub use id::{IdInterner, IdKind, MoveKind, SpeciesKind, TypedId, validate_id};
pub use manifest::{ManifestError, ProjectManifest};
pub use rng::{GameRng, Pcg64Rng, RngState};
pub use version::{Version, VersionReq};

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
