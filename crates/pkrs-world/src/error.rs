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
    /// A map file could not be loaded or breaks the layer conventions.
    #[error(transparent)]
    Map(#[from] crate::importer::MapError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
    /// Error from `pkrs-assets`.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// `maps/world.ron` failed to parse.
    #[error("failed to parse `{file}`: {message}")]
    LayoutParse {
        /// Source file.
        file: String,
        /// Parser message.
        message: String,
    },
    /// `maps/world.ron` failed validation.
    #[error("invalid world layout in `{file}`: {message}")]
    LayoutInvalid {
        /// Source file.
        file: String,
        /// Validation failure.
        message: String,
    },
}
