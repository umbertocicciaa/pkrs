//! Error type for `pkrs-ui` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`UiError`].
pub type Result<T, E = UiError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-ui`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum UiError {
    /// A window skin was malformed.
    #[error("invalid window skin `{0}`")]
    InvalidSkin(String),
    /// Error from `pkrs-render`.
    #[error(transparent)]
    Render(#[from] pkrs_render::RenderError),
    /// Error from `pkrs-platform`.
    #[error(transparent)]
    Platform(#[from] pkrs_platform::PlatformError),
}
