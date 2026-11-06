//! Shared float conversions for font layout.

/// Converts a pixel count (`u32`) to `f32` for layout math.
pub fn u32_px(v: u32) -> f32 {
    v as f32
}

/// Converts a pixel offset (`i32`) to `f32` for layout math.
pub fn i32_px(v: i32) -> f32 {
    v as f32
}

/// Integer text scale as `f32` (minimum 1).
pub fn scale_f(scale: u32) -> f32 {
    scale.max(1) as f32
}
