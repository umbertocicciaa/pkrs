//! Axis-aligned rectangles in pixel space.

/// Rectangle with top-left origin (`x`, `y`) and size (`width`, `height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge in pixels.
    pub x: f32,
    /// Top edge in pixels.
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle from origin and size.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
