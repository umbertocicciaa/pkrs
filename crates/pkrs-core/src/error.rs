//! Error type for `pkrs-core` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`CoreError`].
pub type Result<T, E = CoreError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-core`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CoreError {
    /// An identifier string did not satisfy the id rules.
    #[error("invalid id `{0}`")]
    InvalidId(String),
}
