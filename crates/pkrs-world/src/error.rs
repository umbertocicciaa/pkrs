//! Error type for `pkrs-world` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`WorldError`].
pub type Result<T, E = WorldError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-world`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WorldError {
    /// A map could not be imported.
    #[error("failed to import map `{0}`")]
    MapImport(String),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
    /// Error from `pkrs-assets`.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
}
