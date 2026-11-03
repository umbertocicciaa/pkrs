//! Error type for `pkrs-battle` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`BattleError`].
pub type Result<T, E = BattleError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-battle`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BattleError {
    /// A battle action was not valid in the current battle state.
    #[error("invalid action: {0}")]
    InvalidAction(String),
    /// Error from `pkrs-core`.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// Error from `pkrs-data`.
    #[error(transparent)]
    Data(#[from] pkrs_data::DataError),
}
