//! The [`Animatable`] trait: linear interpolation for animatable types.
//!
//! A [`Tween<T>`](crate::Tween) maps a controller's `0..1` progress to a
//! value of type `T` by calling `T::lerp(from, to, t)`. Any type that
//! can be smoothly interpolated implements [`Animatable`]; the engine
//! ships impls for `f32` and `whisker_css::Color`, with more to follow.

use whisker_css::data_type::{Angle, Color};

/// A type whose values can be linearly interpolated. The fundamental
/// operation a [`Tween`](crate::Tween) needs.
pub trait Animatable: Clone + 'static {
    /// Interpolate from `from` (at `t == 0.0`) to `to` (at `t == 1.0`).
    /// `t` is the eased progress; implementations should treat the
    /// endpoints as exact (`t == 0` ⇒ `from`, `t == 1` ⇒ `to`).
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;
}

impl Animatable for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }
}

/// Linearly interpolate a `u8` channel, rounding to the nearest value.
fn lerp_u8(from: u8, to: u8, t: f32) -> u8 {
    let v = from as f32 + (to as f32 - from as f32) * t;
    v.round().clamp(0.0, 255.0) as u8
}

impl Animatable for Color {
    /// Interpolate two colors.
    ///
    /// - Two `Hsla` colors interpolate in HSL space (hue / saturation /
    ///   lightness / alpha each lerped) — natural for hue sweeps.
    /// - Any pair reducible to concrete RGBA channels (`Rgba`,
    ///   `Transparent`, or an `Hsla`/`Rgba` mix) interpolates per
    ///   channel.
    /// - A [`Color::Named`] endpoint has no resolvable channels here
    ///   (the named-color table lives in Lynx, not in this crate), so a
    ///   pair involving a named color **snaps** at the midpoint rather
    ///   than producing a wrong RGBA. Use explicit `Color::rgb(..)` /
    ///   `Color::hsl(..)` endpoints when you need a smooth color tween.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        // Same-space HSL interpolation keeps hue sweeps natural.
        if let (
            Color::Hsla {
                h: h0,
                s: s0,
                l: l0,
                a: a0,
            },
            Color::Hsla {
                h: h1,
                s: s1,
                l: l1,
                a: a1,
            },
        ) = (from, to)
        {
            return Color::Hsla {
                h: Angle::Deg(f32::lerp(&angle_deg(*h0), &angle_deg(*h1), t)),
                s: f32::lerp(s0, s1, t),
                l: f32::lerp(l0, l1, t),
                a: f32::lerp(a0, a1, t),
            };
        }

        match (rgba_channels(from), rgba_channels(to)) {
            (Some((r0, g0, b0, a0)), Some((r1, g1, b1, a1))) => Color::Rgba(
                lerp_u8(r0, r1, t),
                lerp_u8(g0, g1, t),
                lerp_u8(b0, b1, t),
                f32::lerp(&a0, &a1, t),
            ),
            // A named endpoint can't be resolved to channels here;
            // snap at the midpoint rather than guess.
            _ => {
                if t < 0.5 {
                    *from
                } else {
                    *to
                }
            }
        }
    }
}

/// Resolve a [`Color`] to concrete `(r, g, b, a)` channels, or `None`
/// for a [`Color::Named`] (whose RGB table lives in Lynx, not here).
fn rgba_channels(c: &Color) -> Option<(u8, u8, u8, f32)> {
    match c {
        Color::Rgba(r, g, b, a) => Some((*r, *g, *b, *a)),
        Color::Transparent => Some((0, 0, 0, 0.0)),
        Color::Hsla { h, s, l, a } => {
            let (r, g, b) = hsl_to_rgb(angle_deg(*h), *s / 100.0, *l / 100.0);
            Some((r, g, b, *a))
        }
        Color::Named(_) => None,
    }
}

/// Degrees value of an [`Angle`], for interpolation.
fn angle_deg(a: Angle) -> f32 {
    match a {
        Angle::Deg(d) => d,
        Angle::Rad(r) => r.to_degrees(),
        Angle::Turn(t) => t * 360.0,
    }
}

/// Convert HSL (hue in degrees, saturation/lightness in `0..1`) to
/// 8-bit RGB.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match h {
        _ if h < 60.0 => (c, x, 0.0),
        _ if h < 120.0 => (x, c, 0.0),
        _ if h < 180.0 => (0.0, c, x),
        _ if h < 240.0 => (0.0, x, c),
        _ if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}
