//! Shared float conversions for font layout.

pub(crate) fn u32_px(v: u32) -> f32 {
    v as f32
}

pub(crate) fn i32_px(v: i32) -> f32 {
    v as f32
}

pub(crate) fn scale_f(scale: u32) -> f32 {
    scale.max(1) as f32
}
