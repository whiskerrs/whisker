//! [`AnimConfig`] — the **timing strategy** for an [`AnimationController`].
//!
//! A config carries one of two strategies (see [`Timing`]):
//!
//! - **Curved** — the classic time-driven path: a fixed `duration_ms`
//!   and an easing [`Curve`]. Progress is a *pure function* of elapsed
//!   time (`curve(elapsed / duration)`).
//! - **Spring** — a stateful physics integrator (see [`SpringConfig`]).
//!   There is **no fixed duration**: the spring runs until it settles,
//!   and progress at a given elapsed time is *not* a pure function of
//!   that time (it depends on the hidden velocity). This is why a spring
//!   is **not** a [`Curve`] — you cannot implement `ease(t)` for it.
//!
//! [`AnimationController`]: crate::AnimationController
//! [`Curve`]: crate::Curve

use crate::curve::Curve;

/// Configuration for an animation: which timing strategy drives a
/// controller's `0..1` progress.
///
/// Construct via the curve constructors ([`linear`](Self::linear),
/// [`ease_out`](Self::ease_out), …, [`new`](Self::new)) for the classic
/// time-driven path, or the spring constructors
/// ([`spring`](Self::spring), [`bouncy`](Self::bouncy), …) for
/// physics-based motion.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AnimConfig {
    pub(crate) timing: Timing,
}

/// Which timing strategy an [`AnimConfig`] carries.
///
/// `Curved` and `Spring` are siblings: a curve is a stateless
/// time-to-progress function with a known duration; a spring is a
/// stateful integrator with a hidden velocity and no fixed duration.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum Timing {
    /// The classic time-driven path: progress = `curve(elapsed / dur)`.
    Curved {
        /// Total duration, in milliseconds, for a full `0.0 → 1.0`
        /// sweep. A `reverse()` / partial run scales proportionally.
        duration_ms: f32,
        /// Easing curve applied to the linear time fraction.
        curve: Curve,
    },
    /// Physics-based motion: the controller integrates a position +
    /// velocity toward the target each frame until it settles.
    Spring(SpringConfig),
}

/// Spring (physics) parameters: a damped harmonic oscillator pulling the
/// progress *position* toward its target.
///
/// The controller integrates, per frame,
/// `accel = (-stiffness·(x - target) - damping·velocity) / mass`,
/// stepping velocity then position. Higher `stiffness` is a stronger
/// pull (faster, snappier); higher `damping` removes energy (less
/// overshoot); higher `mass` slows the whole response. There is no
/// duration — the run finishes when position and velocity both settle.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SpringConfig {
    /// Spring constant `k`: strength of the pull toward the target.
    pub stiffness: f32,
    /// Damping coefficient `c`: energy removed per unit velocity.
    pub damping: f32,
    /// Mass `m`: inertia of the animated "object".
    pub mass: f32,
}

impl AnimConfig {
    // ---- curve-based (time-driven) constructors -------------------------

    /// A linear animation over `duration_ms` milliseconds.
    pub fn linear(duration_ms: u32) -> Self {
        Self::curved(duration_ms, Curve::Linear)
    }

    /// An ease-in (accelerate) animation.
    pub fn ease_in(duration_ms: u32) -> Self {
        Self::curved(duration_ms, Curve::EaseIn)
    }

    /// An ease-out (decelerate) animation.
    pub fn ease_out(duration_ms: u32) -> Self {
        Self::curved(duration_ms, Curve::EaseOut)
    }

    /// An ease-in-out animation.
    pub fn ease_in_out(duration_ms: u32) -> Self {
        Self::curved(duration_ms, Curve::EaseInOut)
    }

    /// A custom cubic-Bézier easing `(x1, y1, x2, y2)` (CSS
    /// `cubic-bezier()` model) over `duration_ms`.
    pub fn cubic_bezier(duration_ms: u32, x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self::curved(duration_ms, Curve::CubicBezier(x1, y1, x2, y2))
    }

    /// Build with an explicit [`Curve`].
    pub fn new(duration_ms: u32, curve: Curve) -> Self {
        Self::curved(duration_ms, curve)
    }

    /// Shared constructor for the curve-based path.
    fn curved(duration_ms: u32, curve: Curve) -> Self {
        Self {
            timing: Timing::Curved {
                duration_ms: duration_ms as f32,
                curve,
            },
        }
    }

    // ---- spring (physics) constructors ----------------------------------

    /// A sensible default spring: a gentle, near-critically-damped pull
    /// that settles in a few hundred milliseconds with little to no
    /// overshoot. Defaults are iOS-ish (`stiffness 170, damping 26,
    /// mass 1`).
    pub fn spring() -> Self {
        Self::spring_full(170.0, 26.0, 1.0)
    }

    /// A spring with explicit `stiffness` and `damping`; `mass` is `1.0`.
    pub fn spring_with(stiffness: f32, damping: f32) -> Self {
        Self::spring_full(stiffness, damping, 1.0)
    }

    /// A spring with explicit `stiffness`, `damping`, and `mass`.
    pub fn spring_full(stiffness: f32, damping: f32, mass: f32) -> Self {
        Self {
            timing: Timing::Spring(SpringConfig {
                stiffness,
                damping,
                mass,
            }),
        }
    }

    /// A bouncy, underdamped spring with visible overshoot before it
    /// settles (`stiffness 180, damping 12, mass 1`). Good for playful,
    /// springy motion.
    pub fn bouncy() -> Self {
        Self::spring_full(180.0, 12.0, 1.0)
    }

    /// A stiff, fast spring with minimal overshoot (`stiffness 320,
    /// damping 34, mass 1`). Snappy, near-critically-damped.
    pub fn stiff() -> Self {
        Self::spring_full(320.0, 34.0, 1.0)
    }
}
