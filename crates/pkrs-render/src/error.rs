//! Error type for `pkrs-render` (§15.4).

use thiserror::Error;

/// Convenience alias for results whose error is [`RenderError`].
pub type Result<T, E = RenderError> = std::result::Result<T, E>;

/// Errors produced by `pkrs-render`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RenderError {
    /// The render backend failed.
    #[error("render backend error: {0}")]
    Backend(String),
    /// Error from `pkrs-platform`.
    #[error(transparent)]
    Platform(#[from] pkrs_platform::PlatformError),
    /// Error from `pkrs-assets`.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// RON syntax or schema mismatch in an animation descriptor.
    #[error("animation descriptor parse error: {detail}")]
    AnimationParse {
        /// Parser detail.
        detail: String,
    },
    /// Semantically invalid animation descriptor content.
    #[error("invalid animation descriptor: {detail}")]
    AnimationInvalid {
        /// Validation detail.
        detail: String,
    },
    /// Named clip missing from a sheet.
    #[error("animation clip not found: {name}")]
    AnimationClipNotFound {
        /// Requested clip id.
        name: String,
    },
}
