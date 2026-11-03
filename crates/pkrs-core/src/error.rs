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

    /// A direction string in data did not parse.
    #[error("invalid direction `{0}`")]
    InvalidDirection(String),

    /// The id interner ran out of handles.
    #[error("id interner exhausted (too many unique ids)")]
    IdInternerExhausted,
}
