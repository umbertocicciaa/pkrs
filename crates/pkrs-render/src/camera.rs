//! 2D camera: follow, clamp to bounds, centre small maps, pixel-snapped origin (§5.1, §5.3).
//!
//! Positions are internal pixels. The camera keeps an interpolated (fractional) centre for
//! smoothness, but every draw uses [`Camera::origin`], which is rounded to whole pixels. Entity
//! positions must be rounded the same way (see [`snap`]) so a followed sprite never shimmers.

/// World / map camera: centre point in internal pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Camera {
    /// Interpolated X centre in internal pixels.
    pub x: f32,
    /// Interpolated Y centre in internal pixels.
    pub y: f32,
}

/// Region the camera may show (map bounds; connected maps extend it, §6.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBounds {
    /// Left edge in pixels.
    pub x: f32,
    /// Top edge in pixels.
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
}

impl CameraBounds {
    /// Bounds of a `width` × `height` pixel map at the origin.
    #[must_use]
    pub fn map(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        }
    }
}

/// Rounds a pixel coordinate the same way [`Camera::origin`] does.
#[must_use]
pub fn snap(v: f32) -> i32 {
    v.round() as i32
}

impl Camera {
    /// Creates a camera centred at `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Rounded centre used when issuing draws (avoids sub-pixel shimmer).
    pub fn snapped(&self) -> (i32, i32) {
        (snap(self.x), snap(self.y))
    }

    /// This camera clamped so the view stays inside `bounds`. On an axis where the bounds are
    /// smaller than the view, the camera centres on the bounds instead (§5.3).
    #[must_use]
    pub fn clamped(self, view: (u32, u32), bounds: CameraBounds) -> Self {
        Self {
            x: clamp_axis(self.x, view.0 as f32, bounds.x, bounds.width),
            y: clamp_axis(self.y, view.1 as f32, bounds.y, bounds.height),
        }
    }

    /// Top-left of the `view`, in whole internal pixels: subtract it from a snapped world
    /// position to get the screen position.
    #[must_use]
    pub fn origin(&self, view: (u32, u32)) -> (i32, i32) {
        // Integer half-view keeps `snap(p) - origin` constant for a followed target.
        let half_w = (view.0 / 2) as i32;
        let half_h = (view.1 / 2) as i32;
        let (cx, cy) = self.snapped();
        (cx - half_w, cy - half_h)
    }
}

fn clamp_axis(centre: f32, view: f32, start: f32, len: f32) -> f32 {
    if len <= view {
        start + len / 2.0
    } else {
        let half = view / 2.0;
        centre.clamp(start + half, start + len - half)
    }
}

/// What the camera tracks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraTarget {
    /// The player (default).
    Player,
    /// An entity by stable id (`camera.follow(npc)`).
    Entity(u32),
    /// A fixed point (end of `camera.pan_to`).
    Fixed {
        /// X centre in pixels.
        x: f32,
        /// Y centre in pixels.
        y: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pan {
    from: (f32, f32),
    to: (f32, f32),
    ticks: u32,
    elapsed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Shake {
    amplitude: f32,
    ticks: u32,
    elapsed: u32,
}

/// Tick-driven camera behaviour behind the Lua `camera.*` API (§5.3, §9.1).
///
/// Rust-side stubs for `camera.pan_to(x, y, ticks)`, `camera.follow(npc)` and
/// `camera.shake(amplitude, ticks)`; the Lua bindings call these (#28).
#[derive(Clone, Debug, PartialEq)]
pub struct CameraController {
    target: CameraTarget,
    pan: Option<Pan>,
    shake: Option<Shake>,
    last: (f32, f32),
}

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraController {
    /// Follows the player.
    #[must_use]
    pub fn new() -> Self {
        Self {
            target: CameraTarget::Player,
            pan: None,
            shake: None,
            last: (0.0, 0.0),
        }
    }

    /// Current target.
    #[must_use]
    pub fn target(&self) -> CameraTarget {
        self.target
    }

    /// `true` while a pan is in progress (a script `pan_to` waits on this).
    #[must_use]
    pub fn is_panning(&self) -> bool {
        self.pan.is_some()
    }

    /// `true` while shaking.
    #[must_use]
    pub fn is_shaking(&self) -> bool {
        self.shake.is_some()
    }

    /// `camera.follow(target)`: track `target` from now on (cancels a pan).
    pub fn follow(&mut self, target: CameraTarget) {
        self.target = target;
        self.pan = None;
    }

    /// `camera.pan_to(x, y, ticks)`: move linearly from the last resolved centre to `(x, y)`
    /// over `ticks` ticks, then hold there. `ticks == 0` jumps.
    pub fn pan_to(&mut self, x: f32, y: f32, ticks: u32) {
        self.target = CameraTarget::Fixed { x, y };
        self.pan = (ticks > 0).then_some(Pan {
            from: self.last,
            to: (x, y),
            ticks,
            elapsed: 0,
        });
    }

    /// `camera.shake(amplitude, ticks)`: deterministic shake of up to `amplitude` pixels.
    pub fn shake(&mut self, amplitude: f32, ticks: u32) {
        self.shake = (ticks > 0 && amplitude > 0.0).then_some(Shake {
            amplitude,
            ticks,
            elapsed: 0,
        });
    }

    /// Advances pan and shake by one simulation tick.
    pub fn tick(&mut self) {
        if let Some(pan) = &mut self.pan {
            pan.elapsed = pan.elapsed.saturating_add(1);
            if pan.elapsed >= pan.ticks {
                self.pan = None;
            }
        }
        if let Some(shake) = &mut self.shake {
            shake.elapsed = shake.elapsed.saturating_add(1);
            if shake.elapsed >= shake.ticks {
                self.shake = None;
            }
        }
    }

    /// Resolves the camera for rendering.
    ///
    /// `target_pos` is the interpolated centre of the current target (the player or the followed
    /// entity), or `None` for [`CameraTarget::Fixed`]. The result is clamped to `bounds` and then
    /// shaken (so shakes stay visible at map edges).
    pub fn resolve(
        &mut self,
        target_pos: Option<(f32, f32)>,
        alpha: f32,
        view: (u32, u32),
        bounds: CameraBounds,
    ) -> Camera {
        let centre = match (self.pan, self.target) {
            (Some(pan), _) => {
                let t = ((pan.elapsed as f32 + alpha) / pan.ticks as f32).clamp(0.0, 1.0);
                (
                    pan.from.0 + (pan.to.0 - pan.from.0) * t,
                    pan.from.1 + (pan.to.1 - pan.from.1) * t,
                )
            }
            (None, CameraTarget::Fixed { x, y }) => (x, y),
            (None, _) => target_pos.unwrap_or(self.last),
        };
        self.last = centre;
        let camera = Camera::new(centre.0, centre.1).clamped(view, bounds);
        let (dx, dy) = self.shake_offset();
        Camera::new(camera.x + dx, camera.y + dy)
    }

    /// Current shake offset: alternates sign every tick and decays linearly to zero.
    #[must_use]
    pub fn shake_offset(&self) -> (f32, f32) {
        let Some(shake) = self.shake else {
            return (0.0, 0.0);
        };
        let remaining = 1.0 - shake.elapsed as f32 / shake.ticks as f32;
        let magnitude = (shake.amplitude * remaining).round();
        let sign = if shake.elapsed % 2 == 0 { 1.0 } else { -1.0 };
        let vertical = if shake.elapsed % 4 < 2 { 1.0 } else { -1.0 };
        (magnitude * sign, magnitude * vertical * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: (u32, u32) = (240, 160);

    #[test]
    fn snapped_rounds_to_whole_pixels() {
        let cam = Camera::new(10.4, 20.6);
        assert_eq!(cam.snapped(), (10, 21));
    }

    #[test]
    fn clamp_keeps_view_inside_large_map() {
        let bounds = CameraBounds::map(640, 480);
        // Near the top-left corner: clamped to half the view.
        let cam = Camera::new(10.0, 10.0).clamped(VIEW, bounds);
        assert_eq!((cam.x, cam.y), (120.0, 80.0));
        assert_eq!(cam.origin(VIEW), (0, 0));
        // Bottom-right corner.
        let cam = Camera::new(630.0, 470.0).clamped(VIEW, bounds);
        assert_eq!(cam.origin(VIEW), (400, 320));
        // Middle: untouched.
        let cam = Camera::new(300.0, 200.0).clamped(VIEW, bounds);
        assert_eq!((cam.x, cam.y), (300.0, 200.0));
    }

    #[test]
    fn small_map_is_centred() {
        // 160×96 map in a 240×160 view: centred on both axes.
        let cam = Camera::new(0.0, 0.0).clamped(VIEW, CameraBounds::map(160, 96));
        assert_eq!((cam.x, cam.y), (80.0, 48.0));
        assert_eq!(cam.origin(VIEW), (-40, -32));
        // Narrow but tall map: centred on X only.
        let cam = Camera::new(500.0, 500.0).clamped(VIEW, CameraBounds::map(128, 640));
        assert_eq!((cam.x, cam.y), (64.0, 500.0));
    }

    #[test]
    fn followed_target_has_constant_screen_position() {
        // No sub-pixel shimmer: while not clamped, the player's snapped screen position is the
        // same for every fractional world position along a step.
        let bounds = CameraBounds::map(1600, 1600);
        let mut screen = Vec::new();
        for i in 0..=16 {
            let px = 400.0 + i as f32 * (16.0 / 16.0) + 0.37;
            let py = 600.0 - i as f32 * 0.61;
            let cam = Camera::new(px, py).clamped(VIEW, bounds);
            let (ox, oy) = cam.origin(VIEW);
            screen.push((snap(px) - ox, snap(py) - oy));
        }
        assert!(screen.iter().all(|&s| s == (120, 80)), "{screen:?}");
    }

    #[test]
    fn controller_follows_player_by_default() {
        let mut ctl = CameraController::new();
        let cam = ctl.resolve(
            Some((300.0, 300.0)),
            0.0,
            VIEW,
            CameraBounds::map(1000, 1000),
        );
        assert_eq!((cam.x, cam.y), (300.0, 300.0));
    }

    #[test]
    fn pan_interpolates_over_ticks_then_holds() {
        let bounds = CameraBounds::map(2000, 2000);
        let mut ctl = CameraController::new();
        ctl.resolve(Some((200.0, 200.0)), 0.0, VIEW, bounds);
        ctl.pan_to(400.0, 200.0, 4);
        assert!(ctl.is_panning());
        let mut xs = Vec::new();
        for _ in 0..4 {
            xs.push(ctl.resolve(Some((200.0, 200.0)), 0.0, VIEW, bounds).x);
            ctl.tick();
        }
        assert_eq!(xs, [200.0, 250.0, 300.0, 350.0]);
        assert!(!ctl.is_panning());
        assert_eq!(ctl.resolve(Some((0.0, 0.0)), 0.0, VIEW, bounds).x, 400.0);
        ctl.follow(CameraTarget::Player);
        assert_eq!(
            ctl.resolve(Some((500.0, 500.0)), 0.0, VIEW, bounds).x,
            500.0
        );
    }

    #[test]
    fn shake_is_deterministic_and_decays() {
        let mut ctl = CameraController::new();
        ctl.shake(4.0, 4);
        let mut offsets = Vec::new();
        while ctl.is_shaking() {
            offsets.push(ctl.shake_offset());
            ctl.tick();
        }
        assert_eq!(
            offsets,
            [(4.0, 2.0), (-3.0, 1.5), (2.0, -1.0), (-1.0, -0.5)]
        );
        assert_eq!(ctl.shake_offset(), (0.0, 0.0));
    }
}
