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

    /// A virtual path is malformed (absolute, forbidden characters, bad mount prefix, …).
    #[error("invalid asset path `{path}`: {reason}")]
    InvalidPath {
        /// The rejected path as written.
        path: String,
        /// Why it was rejected.
        reason: String,
    },

    /// A path tried to leave its mount root (`..` past the root, or a symlink pointing outside).
    #[error("asset path `{0}` escapes the project root")]
    PathEscapesRoot(String),

    /// The file exists but with different letter case; paths are case-sensitive on every OS.
    #[error("asset path `{requested}` does not match the on-disk case `{actual}`")]
    CaseMismatch {
        /// The path as requested.
        requested: String,
        /// The path with the case found on disk.
        actual: String,
    },

    /// A `plugin_id:` prefix names a mount that does not exist.
    #[error("no mount for prefix `{0}:`")]
    UnknownMount(String),

    /// A mount with this prefix already exists.
    #[error("mount prefix `{0}:` is already in use")]
    DuplicateMount(String),

    /// The underlying storage failed.
    #[error("cannot read `{path}`: {message}")]
    Io {
        /// The virtual path being read.
        path: String,
        /// Description of the I/O failure.
        message: String,
    },

    /// The bytes were read but could not be decoded into the requested asset type.
    #[error("cannot decode `{path}`: {message}")]
    Decode {
        /// The virtual path being decoded.
        path: String,
        /// Why decoding failed.
        message: String,
    },

    /// A handle refers to an asset that has been evicted (or belongs to another store).
    #[error("stale asset handle")]
    StaleHandle,

    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}
