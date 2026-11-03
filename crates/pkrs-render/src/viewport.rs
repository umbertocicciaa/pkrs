//! Integer-scale and fit viewport math (§5.1).

use crate::rect::Rect;

/// How the internal render target is scaled to the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewportMode {
    /// Largest integer scale that fits, centered with letterbox (default).
    #[default]
    Integer,
    /// Non-integer uniform scale that fits (still nearest-filtered).
    Fit,
}

/// Where the internal image is drawn on the window, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportLayout {
    /// Uniform scale factor from internal pixels to screen pixels.
    pub scale: f32,
    /// Destination rectangle on the window (centered letterbox).
    pub dest: Rect,
}

/// Computes viewport layout for `internal` drawn into `window` (§5.1).
pub fn compute_viewport(
    internal_width: u32,
    internal_height: u32,
    window_width: u32,
    window_height: u32,
    mode: ViewportMode,
) -> ViewportLayout {
    if internal_width == 0 || internal_height == 0 || window_width == 0 || window_height == 0 {
        return ViewportLayout {
            scale: 1.0,
            dest: Rect::new(0.0, 0.0, 0.0, 0.0),
        };
    }

    let (scale, dest_w, dest_h) = match mode {
        ViewportMode::Integer => {
            let scale_x = window_width / internal_width;
            let scale_y = window_height / internal_height;
            let scale_u = scale_x.min(scale_y);
            let dest_w = internal_width.saturating_mul(scale_u);
            let dest_h = internal_height.saturating_mul(scale_u);
            (scale_u as f32, dest_w as f32, dest_h as f32)
        }
        ViewportMode::Fit => {
            let scale_x = window_width as f32 / internal_width as f32;
            let scale_y = window_height as f32 / internal_height as f32;
            let scale = scale_x.min(scale_y);
            let dest_w = internal_width as f32 * scale;
            let dest_h = internal_height as f32 * scale;
            (scale, dest_w, dest_h)
        }
    };

    let dest_x = (window_width as f32 - dest_w) / 2.0;
    let dest_y = (window_height as f32 - dest_h) / 2.0;

    ViewportLayout {
        scale,
        dest: Rect::new(dest_x, dest_y, dest_w, dest_h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_scale_fills_720x480_for_240x160() {
        let v = compute_viewport(240, 160, 720, 480, ViewportMode::Integer);
        assert_eq!(v.scale, 3.0);
        assert_eq!(v.dest, Rect::new(0.0, 0.0, 720.0, 480.0));
    }

    #[test]
    fn integer_scale_centers_on_odd_window() {
        let v = compute_viewport(240, 160, 241, 161, ViewportMode::Integer);
        assert_eq!(v.scale, 1.0);
        assert_eq!(v.dest.x, 0.5);
        assert_eq!(v.dest.y, 0.5);
        assert_eq!(v.dest.width, 240.0);
        assert_eq!(v.dest.height, 160.0);
    }

    #[test]
    fn fit_mode_uses_fractional_scale() {
        let v = compute_viewport(240, 160, 500, 500, ViewportMode::Fit);
        assert!(v.scale > 2.0 && v.scale < 3.0);
        assert!(v.dest.width <= 500.0);
        assert!(v.dest.height <= 500.0);
    }

    #[test]
    fn table_of_window_sizes() {
        let internal = (240_u32, 160_u32);
        for (win_w, win_h, expect_scale) in [
            (720_u32, 480_u32, 3.0),
            (480_u32, 320_u32, 2.0),
            (240_u32, 160_u32, 1.0),
            (800_u32, 600_u32, 3.0),
        ] {
            let v = compute_viewport(internal.0, internal.1, win_w, win_h, ViewportMode::Integer);
            assert_eq!(v.scale, expect_scale, "window {win_w}x{win_h}");
            assert_eq!(v.dest.width, internal.0 as f32 * expect_scale);
            assert_eq!(v.dest.height, internal.1 as f32 * expect_scale);
        }
    }
}
