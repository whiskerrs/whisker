//! [`AnimConfig`] — duration + easing for an [`AnimationController`].
//!
//! [`AnimationController`]: crate::AnimationController

use crate::curve::Curve;

/// Configuration for a time-driven animation: how long it runs and how
/// progress is shaped over that time.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AnimConfig {
    /// Total duration, in milliseconds, for a full `0.0 → 1.0` sweep.
    /// A `reverse()` / partial run scales proportionally (the time to
    /// cover the remaining distance at the same rate).
    pub duration_ms: f32,
    /// Easing curve applied to the linear time fraction.
    pub curve: Curve,
}

impl AnimConfig {
    /// A linear animation over `duration_ms` milliseconds.
    pub fn linear(duration_ms: u32) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve: Curve::Linear,
        }
    }

    /// An ease-in (accelerate) animation.
    pub fn ease_in(duration_ms: u32) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve: Curve::EaseIn,
        }
    }

    /// An ease-out (decelerate) animation.
    pub fn ease_out(duration_ms: u32) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve: Curve::EaseOut,
        }
    }

    /// An ease-in-out animation.
    pub fn ease_in_out(duration_ms: u32) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve: Curve::EaseInOut,
        }
    }

    /// A custom cubic-Bézier easing `(x1, y1, x2, y2)` (CSS
    /// `cubic-bezier()` model) over `duration_ms`.
    pub fn cubic_bezier(duration_ms: u32, x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve: Curve::CubicBezier(x1, y1, x2, y2),
        }
    }

    /// Build with an explicit [`Curve`].
    pub fn new(duration_ms: u32, curve: Curve) -> Self {
        Self {
            duration_ms: duration_ms as f32,
            curve,
        }
    }
}
