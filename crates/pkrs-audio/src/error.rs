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
    /// No audio output device could be opened.
    #[error("no audio output device: {0}")]
    NoDevice(String),
    /// The audio manifest (`audio/bgm/index.ron`) is malformed.
    #[error("{file}: {message}")]
    Manifest {
        /// Manifest path.
        file: String,
        /// What is wrong (RON position included).
        message: String,
    },
    /// A sound file could not be decoded.
    #[error("cannot decode `{path}`: {message}")]
    Decode {
        /// VFS path of the sound.
        path: String,
        /// Decoder message.
        message: String,
    },
    /// Error from `pkrs-assets`.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
}
