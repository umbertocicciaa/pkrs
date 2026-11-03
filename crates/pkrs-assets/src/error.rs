//! Error type for `pkrs-assets` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`AssetsError`].
pub type Result<T, E = AssetsError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-assets`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AssetsError {
    /// No asset exists at the requested virtual path.
    #[error("asset not found: {0}")]
    NotFound(String),
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}
