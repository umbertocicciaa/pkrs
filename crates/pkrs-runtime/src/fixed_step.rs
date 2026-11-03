//! Fixed-step accumulator and render interpolation factor (§3.1).

/// Simulation tick duration in seconds (60 Hz).
pub const TICK_DT: f64 = 1.0 / 60.0;

/// Advances wall time into discrete ticks and a render [`alpha`](Self::alpha).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FixedStepClock {
    acc: f64,
}

impl FixedStepClock {
    /// Creates a clock with zero accumulated time.
    pub const fn new() -> Self {
        Self { acc: 0.0 }
    }

    /// Adds `frame_time` seconds (already clamped by [`Platform`](pkrs_platform::Platform)).
    ///
    /// Returns how many simulation ticks to run this frame.
    pub fn advance(&mut self, frame_time: f64) -> u32 {
        if frame_time.is_finite() && frame_time > 0.0 {
            self.acc += frame_time;
        }
        let mut ticks = 0u32;
        while self.acc >= TICK_DT {
            self.acc -= TICK_DT;
            ticks = ticks.saturating_add(1);
        }
        ticks
    }

    /// Interpolation factor in `[0, 1)` between the previous and current tick (§3.1).
    pub fn alpha(&self) -> f32 {
        if TICK_DT > 0.0 {
            (self.acc / TICK_DT) as f32
        } else {
            0.0
        }
    }

    /// Remaining accumulated time after the last [`advance`](Self::advance).
    pub fn accumulator(&self) -> f64 {
        self.acc
    }
}

/// Linear interpolation for render extrapolation between ticks.
#[must_use]
pub fn lerp_f32(prev: f32, curr: f32, alpha: f32) -> f32 {
    prev + (curr - prev) * alpha
}

/// Position pair updated once per simulation tick (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct InterpolatedPosition {
    /// Position at the end of the previous tick.
    pub prev: (f32, f32),
    /// Position at the end of the current tick.
    pub pos: (f32, f32),
}

impl InterpolatedPosition {
    /// Sets a new logical position, moving the old `pos` into `prev`.
    pub fn set(&mut self, x: f32, y: f32) {
        self.prev = self.pos;
        self.pos = (x, y);
    }

    /// Render position for interpolation factor `alpha`.
    pub fn at(&self, alpha: f32) -> (f32, f32) {
        (
            lerp_f32(self.prev.0, self.pos.0, alpha),
            lerp_f32(self.prev.1, self.pos.1, alpha),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_ticks(clock: &mut FixedStepClock, frame_times: &[f64]) -> u32 {
        frame_times.iter().map(|dt| clock.advance(*dt)).sum()
    }

    #[test]
    fn sixty_hz_frames_yield_one_tick_each() {
        let mut clock = FixedStepClock::new();
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            assert_eq!(clock.advance(dt), 1);
            assert!(clock.alpha() < f32::EPSILON);
        }
        assert_eq!(total_ticks(&mut FixedStepClock::new(), &vec![dt; 60]), 60);
    }

    #[test]
    fn thirty_hz_over_one_second_yields_sixty_ticks() {
        let dt = 1.0 / 30.0;
        let frames: Vec<_> = std::iter::repeat_n(dt, 30).collect();
        assert_eq!(total_ticks(&mut FixedStepClock::new(), &frames), 60);
    }

    #[test]
    fn one_forty_four_hz_over_one_second_yields_sixty_ticks() {
        let step = 1.0 / 144.0;
        let mut frame_times: Vec<f64> = std::iter::repeat_n(step, 143).collect();
        frame_times.push(1.0 - 143.0 * step);
        assert_eq!(total_ticks(&mut FixedStepClock::new(), &frame_times), 60);
    }

    #[test]
    fn max_clamped_frame_time_yields_fifteen_ticks() {
        let mut clock = FixedStepClock::new();
        assert_eq!(clock.advance(0.25), 15);
        assert_eq!(clock.advance(0.25), 15);
    }

    #[test]
    fn alpha_tracks_leftover_time() {
        let mut clock = FixedStepClock::new();
        assert_eq!(clock.advance(1.0 / 120.0), 0);
        let alpha = clock.alpha();
        assert!((alpha - 0.5).abs() < 1e-5);
    }

    #[test]
    fn interpolated_position_lerp() {
        let mut p = InterpolatedPosition::default();
        p.set(0.0, 0.0);
        p.set(10.0, 20.0);
        let mid = p.at(0.5);
        assert!((mid.0 - 5.0).abs() < 1e-5);
        assert!((mid.1 - 10.0).abs() < 1e-5);
    }
}
