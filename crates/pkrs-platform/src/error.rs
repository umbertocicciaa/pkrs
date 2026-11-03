//! Error type for `pkrs-platform` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`PlatformError`].
pub type Result<T, E = PlatformError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-platform`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PlatformError {
    /// The platform backend failed.
    #[error("platform backend error: {0}")]
    Backend(String),
    /// Per-OS user directories could not be resolved.
    #[error("user directories are unavailable")]
    UserPathsUnavailable,
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}
