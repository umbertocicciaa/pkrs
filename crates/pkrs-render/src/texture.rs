//! Texture handles returned by the render backend.

/// Opaque handle to a GPU/uploaded texture in the current backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureHandle(pub u32);
