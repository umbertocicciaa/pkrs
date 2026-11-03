//! RGBA colours in engine vocabulary.

/// An 8-bit-per-channel colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel (`0`–`255`).
    pub r: u8,
    /// Green channel (`0`–`255`).
    pub g: u8,
    /// Blue channel (`0`–`255`).
    pub b: u8,
    /// Alpha channel (`0`–`255`).
    pub a: u8,
}

impl Color {
    /// Opaque black (default letterbox bars, §5.1).
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
}
