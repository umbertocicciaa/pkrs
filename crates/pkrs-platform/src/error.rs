//! Error type for `pkrs-platform` (§15.4).

use std::path::PathBuf;

use thiserror::Error;

/// Convenience alias for results whose error is [`PlatformError`].
pub type Result<T, E = PlatformError> = std::result::Result<T, E>;

/// Convenience alias for input facade results.
pub type InputResult<T> = std::result::Result<T, InputError>;

/// Errors from the [`Input`](crate::Input) facade and action registry.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum InputError {
    /// The action name is not valid `snake_case` or `plugin:action`.
    #[error("invalid action name `{0}`")]
    InvalidActionName(String),
    /// The action was already registered.
    #[error("action `{0}` is already registered")]
    DuplicateAction(String),
    /// Too many custom actions were registered.
    #[error("custom action registry is full")]
    RegistryFull,
    /// A manifest or config key name is unknown.
    #[error("unknown key name `{key}` for action `{action}`")]
    UnknownKeyName {
        /// Action table entry in `pkrs.toml`.
        action: String,
        /// Key string that could not be parsed.
        key: String,
    },
    /// A binding lists no keys.
    #[error("action `{0}` must list at least one key")]
    EmptyBinding(String),
}

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
    /// A `game.id` could not be turned into per-OS user directories.
    #[error("invalid game id `{0}` (expected e.g. `com.example.game`)")]
    InvalidGameId(String),
    /// A file requested through the platform does not exist.
    #[error("file not found: `{}`", .0.display())]
    FileNotFound(PathBuf),
    /// Any other I/O failure on a platform file operation.
    #[error("I/O error on `{}`", path.display())]
    Io {
        /// Path of the file being read or written.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: std::io::Error,
    },
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
}

impl PlatformError {
    #[cfg(feature = "macroquad")]
    /// Wraps an OS I/O error for `path`, mapping "not found" to [`PlatformError::FileNotFound`].
    pub(crate) fn from_io(path: &std::path::Path, source: std::io::Error) -> Self {
        if source.kind() == std::io::ErrorKind::NotFound {
            Self::FileNotFound(path.to_path_buf())
        } else {
            Self::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    }
}
