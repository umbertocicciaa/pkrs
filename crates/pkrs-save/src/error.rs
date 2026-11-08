//! Error type for `pkrs-save` (§15.4).

use std::path::PathBuf;

use thiserror::Error;

/// Convenience alias for results whose error is [`SaveError`].
pub type Result<T, E = SaveError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-save`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SaveError {
    /// The save file uses a format version this build cannot read.
    #[error("unsupported save version {0}")]
    UnsupportedVersion(u32),
    /// The save has no readable `header.format_version`.
    #[error("save has no `header.format_version`")]
    MissingVersion,
    /// No migration step is registered from this format version.
    #[error("no save migration registered from format version {0}")]
    MissingMigration(u32),
    /// An engine migration step failed.
    #[error("save migration v{from} -> v{to} failed: {message}")]
    Migration {
        /// Version the step migrates from.
        from: u32,
        /// Version the step migrates to.
        to: u32,
        /// What went wrong.
        message: String,
    },
    /// The game's `scripts/migrate.lua` failed or returned something that is not a save.
    #[error("game save migration failed: {0}")]
    GameMigration(String),
    /// The save text is not valid RON, or a value could not be written as RON.
    #[error("save file is not valid: {0}")]
    Codec(String),
    /// The save's value tree does not match the save schema.
    #[error("save does not match the save schema: {0}")]
    Schema(String),
    /// Reading or writing a save file failed.
    #[error("save file `{path}`: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The slot is not one of the store's slots.
    #[error("this store has no {0}")]
    UnknownSlot(String),
    /// The slot has no save (neither the file nor its backup exists).
    #[error("no save in `{0}`")]
    NotFound(PathBuf),
    /// Neither the save nor its backup could be loaded.
    #[error("save `{path}` cannot be loaded ({main}); its backup failed too ({backup})")]
    Unrecoverable {
        /// The main save file.
        path: PathBuf,
        /// Why the main file failed.
        main: Box<SaveError>,
        /// Why the backup failed.
        backup: Box<SaveError>,
    },
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
}

impl SaveError {
    /// An [`Io`](Self::Io) error for `path`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// `true` when the error means "this file does not exist".
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        match self {
            Self::NotFound(_) => true,
            Self::Io { source, .. } => source.kind() == std::io::ErrorKind::NotFound,
            _ => false,
        }
    }
}
