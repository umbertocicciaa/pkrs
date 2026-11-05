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
    /// Duplicate id in a content bundle.
    #[error("duplicate {kind} `{id}`")]
    Duplicate {
        /// Kind of record (move, species, …).
        kind: String,
        /// Duplicate id.
        id: String,
    },
    /// Invalid field value in content data.
    #[error("invalid {field} in `{context}`: {message}")]
    InvalidField {
        /// Field name.
        field: String,
        /// Record or table context.
        context: String,
        /// Why the value is invalid.
        message: String,
    },
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}
