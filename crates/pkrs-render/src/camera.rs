//! 2D camera with interpolated position and pixel-snapped drawing (§5.3).

/// World / map camera in internal pixel space.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Camera {
    /// Interpolated X centre in internal pixels.
    pub x: f32,
    /// Interpolated Y centre in internal pixels.
    pub y: f32,
}

impl Camera {
    /// Creates a camera centred at `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Rounded centre used when issuing draws (avoids sub-pixel shimmer).
    pub fn snapped(&self) -> (i32, i32) {
        (self.x.round() as i32, self.y.round() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapped_rounds_to_whole_pixels() {
        let cam = Camera::new(10.4, 20.6);
        assert_eq!(cam.snapped(), (10, 21));
    }
}
