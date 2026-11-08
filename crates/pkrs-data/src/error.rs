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
    /// A content id is defined twice and the later record does not declare `override`
    /// (§8.1, §9.4).
    #[error(
        "duplicate {kind} `{id}`: already defined by {existing}; {by} must declare `override: true` to replace it"
    )]
    DuplicateContent {
        /// Kind of content (`move`, `terrain`, …).
        kind: String,
        /// The repeated id.
        id: String,
        /// Who defined it first.
        existing: String,
        /// Who tried to define it again.
        by: String,
    },
    /// The same id is defined twice in the game's data folders (and the later record does
    /// not declare `override: true`).
    #[error("duplicate {kind} `{id}` in `{file}` (already defined in `{first}`)")]
    DuplicateId {
        /// Kind of record (`species`, `move`, `text key`, …).
        kind: String,
        /// The repeated id.
        id: String,
        /// File of the second definition.
        file: String,
        /// File (or source) of the first definition.
        first: String,
    },
    /// A data file could not be read.
    #[error("cannot read `{file}`: {message}")]
    Read {
        /// Content file path, relative to the project root.
        file: String,
        /// Why reading failed.
        message: String,
    },
    /// A problem in a record of `file` (the inner error names the record).
    #[error("{file}: {source}")]
    InFile {
        /// Content file path, relative to the project root.
        file: String,
        /// The problem.
        source: Box<DataError>,
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
