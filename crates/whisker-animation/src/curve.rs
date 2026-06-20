//! Easing curves: pure `f32 -> f32` mappings over the unit interval.
//!
//! A [`Curve`] takes a linear time fraction `t ∈ [0, 1]` (elapsed /
//! duration) and returns an eased progress in `[0, 1]`. Every curve
//! satisfies `f(0) == 0` and `f(1) == 1`; intermediate shaping is what
//! distinguishes them.

/// An easing curve. Cheap to copy; pure (no state).
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Curve {
    /// No easing — progress equals time.
    Linear,
    /// Cubic accelerate-from-zero (`t³`).
    EaseIn,
    /// Cubic decelerate-to-one (`1 - (1-t)³`).
    EaseOut,
    /// Cubic ease-in then ease-out, symmetric about `t = 0.5`.
    EaseInOut,
    /// Arbitrary cubic Bézier `(x1, y1, x2, y2)` with implicit
    /// endpoints `(0,0)` and `(1,1)` — the CSS `cubic-bezier()` model.
    CubicBezier(f32, f32, f32, f32),
}

impl Curve {
    /// Evaluate the curve at time fraction `t`. `t` is clamped to
    /// `[0, 1]` first, so callers needn't pre-clamp.
    pub fn ease(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::EaseIn => t * t * t,
            Curve::EaseOut => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Curve::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let u = -2.0 * t + 2.0;
                    1.0 - (u * u * u) / 2.0
                }
            }
            Curve::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, t),
        }
    }
}

/// Evaluate a CSS-style cubic Bézier easing with control points
/// `(x1, y1)` and `(x2, y2)` (endpoints fixed at `(0,0)` and `(1,1)`).
///
/// The curve is parametric in `s`: `x(s)` and `y(s)` are cubic Béziers.
/// We solve `x(s) = t` for the parameter `s` (Newton's method, with a
/// bisection fallback for robustness), then return `y(s)`.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    // Bézier basis for endpoints (0,0)..(1,1): the cubic in `s` reduces
    // to `3(1-s)²s·c1 + 3(1-s)s²·c2 + s³` for a control coordinate `c`.
    fn bezier(c1: f32, c2: f32, s: f32) -> f32 {
        let u = 1.0 - s;
        3.0 * u * u * s * c1 + 3.0 * u * s * s * c2 + s * s * s
    }
    fn bezier_deriv(c1: f32, c2: f32, s: f32) -> f32 {
        let u = 1.0 - s;
        3.0 * u * u * c1 + 6.0 * u * s * (c2 - c1) + 3.0 * s * s * (1.0 - c2)
    }

    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    // Newton-Raphson to invert x(s) = t.
    let mut s = t;
    for _ in 0..8 {
        let x = bezier(x1, x2, s) - t;
        if x.abs() < 1e-6 {
            return bezier(y1, y2, s);
        }
        let dx = bezier_deriv(x1, x2, s);
        if dx.abs() < 1e-6 {
            break;
        }
        s -= x / dx;
    }

    // Bisection fallback if Newton stalled or shot out of range.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    let mut s = t.clamp(lo, hi);
    for _ in 0..32 {
        let x = bezier(x1, x2, s);
        if (x - t).abs() < 1e-6 {
            break;
        }
        if x < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    bezier(y1, y2, s)
}
