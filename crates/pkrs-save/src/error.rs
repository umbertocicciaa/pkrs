//! Error type for `pkrs-save` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`SaveError`].
pub type Result<T, E = SaveError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-save`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SaveError {
    /// The save file uses a format version this build cannot read.
    #[error("unsupported save version {0}")]
    UnsupportedVersion(u32),
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
}
