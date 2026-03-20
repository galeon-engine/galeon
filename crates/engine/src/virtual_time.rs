// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

/// Virtual time controls: pause, speed scaling, and max-delta clamping.
///
/// Insert this resource to gain time control. If absent, the game loop
/// uses raw host elapsed (backward compatible).
///
/// # Example
///
/// ```rust
/// use galeon_engine::VirtualTime;
///
/// let mut vt = VirtualTime::new();
/// assert!((vt.effective_elapsed(0.5) - 0.25).abs() < f64::EPSILON); // clamped to max_delta
///
/// vt.paused = true;
/// assert_eq!(vt.effective_elapsed(1.0), 0.0);
///
/// vt.paused = false;
/// vt.scale = 2.0;
/// assert!((vt.effective_elapsed(0.1) - 0.2).abs() < f64::EPSILON);
/// ```
pub struct VirtualTime {
    /// When true, `effective_elapsed()` always returns 0.
    pub paused: bool,
    /// Speed multiplier. Clamped to `[0.0, 8.0]`.
    /// - 1.0 = normal
    /// - 0.5 = half speed
    /// - 2.0 = double speed
    pub scale: f64,
    /// Raw elapsed is clamped to this value before scaling.
    /// Prevents death spirals when the host delivers a huge frame delta
    /// (e.g., tab was backgrounded). Default: 0.25 s.
    pub max_delta: f64,
    /// Total virtual time elapsed since engine start.
    /// Accumulated by `game_loop::tick()` each frame.
    pub elapsed: f64,
}

impl VirtualTime {
    /// Create with default settings: unpaused, scale 1.0, max_delta 0.25 s.
    pub fn new() -> Self {
        Self {
            paused: false,
            scale: 1.0,
            max_delta: 0.25,
            elapsed: 0.0,
        }
    }

    /// Transform raw host elapsed into virtual elapsed.
    ///
    /// Returns 0.0 when paused. Otherwise clamps `raw` to `max_delta`,
    /// then multiplies by `scale` (clamped to `[0.0, 8.0]`).
    pub fn effective_elapsed(&self, raw: f64) -> f64 {
        if self.paused {
            return 0.0;
        }
        let clamped = raw.min(self.max_delta).max(0.0);
        let scale = self.scale.clamp(0.0, 8.0);
        clamped * scale
    }
}

impl Default for VirtualTime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let vt = VirtualTime::new();
        assert!(!vt.paused);
        assert!((vt.scale - 1.0).abs() < f64::EPSILON);
        assert!((vt.max_delta - 0.25).abs() < f64::EPSILON);
        assert!((vt.elapsed - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_elapsed_normal() {
        let vt = VirtualTime::new();
        let e = vt.effective_elapsed(0.1);
        assert!((e - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_elapsed_paused() {
        let mut vt = VirtualTime::new();
        vt.paused = true;
        assert_eq!(vt.effective_elapsed(1.0), 0.0);
        assert_eq!(vt.effective_elapsed(0.0), 0.0);
    }

    #[test]
    fn effective_elapsed_scaled() {
        let mut vt = VirtualTime::new();
        vt.scale = 2.0;
        let e = vt.effective_elapsed(0.1);
        assert!((e - 0.2).abs() < f64::EPSILON);

        vt.scale = 0.5;
        let e = vt.effective_elapsed(0.1);
        assert!((e - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_elapsed_max_delta_clamp() {
        let vt = VirtualTime::new(); // max_delta = 0.25
        let e = vt.effective_elapsed(2.0); // clamped to 0.25
        assert!((e - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_elapsed_scale_and_clamp_interact() {
        let mut vt = VirtualTime::new(); // max_delta = 0.25
        vt.scale = 4.0;
        let e = vt.effective_elapsed(2.0); // clamped to 0.25, then * 4.0 = 1.0
        assert!((e - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scale_clamped_to_bounds() {
        let mut vt = VirtualTime::new();
        vt.scale = -1.0;
        assert_eq!(vt.effective_elapsed(0.1), 0.0); // scale clamped to 0.0

        vt.scale = 100.0;
        let e = vt.effective_elapsed(0.1);
        assert!((e - 0.8).abs() < f64::EPSILON); // scale clamped to 8.0, 0.1 * 8.0
    }

    #[test]
    fn negative_raw_clamped_to_zero() {
        let vt = VirtualTime::new();
        assert_eq!(vt.effective_elapsed(-0.5), 0.0);
    }
}
