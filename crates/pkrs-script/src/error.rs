//! Error type for `pkrs-script` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`ScriptError`].
pub type Result<T, E = ScriptError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-script`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ScriptError {
    /// A script raised an error.
    #[error("script error in `{chunk}`: {message}")]
    Runtime {
        /// Script chunk name (usually its path).
        chunk: String,
        /// Error message.
        message: String,
    },
    /// Error from `pkrs-world`.
    #[error(transparent)]
    World(#[from] pkrs_world::WorldError),
    /// Error from `pkrs-battle`.
    #[error(transparent)]
    Battle(#[from] pkrs_battle::BattleError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
    /// Error from `pkrs-save`.
    #[error(transparent)]
    Save(#[from] pkrs_save::SaveError),
    /// Error from `pkrs-ui`.
    #[error(transparent)]
    Ui(#[from] pkrs_ui::UiError),
    /// Error from `pkrs-audio`.
    #[error(transparent)]
    Audio(#[from] pkrs_audio::AudioError),
}
