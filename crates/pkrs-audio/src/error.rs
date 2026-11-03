//! Error type for `pkrs-audio` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`AudioError`].
pub type Result<T, E = AudioError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-audio`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AudioError {
    /// The audio backend failed.
    #[error("audio backend error: {0}")]
    Backend(String),
    /// Error from `pkrs-assets`.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
}
