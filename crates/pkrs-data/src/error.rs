//! Error type for `pkrs-data` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`DataError`].
pub type Result<T, E = DataError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-data`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DataError {
    /// A content file could not be parsed.
    #[error("failed to parse `{file}`: {message}")]
    Parse {
        /// Content file path, relative to the project root.
        file: String,
        /// Parser message.
        message: String,
    },
    /// A record references an id that does not exist.
    #[error("unknown {kind} `{id}`")]
    UnknownReference {
        /// Kind of record referenced (species, move, item, …).
        kind: String,
        /// The missing id.
        id: String,
    },
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}
