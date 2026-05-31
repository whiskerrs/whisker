//! `<color>` — colors in their various CSS forms.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/color.html>
//!
//! Lynx accepts:
//!
//! - 147 [`NamedColor`]s (the CSS named-color set).
//! - `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` hexadecimal triplets.
//! - The `rgb()`, `rgba()`, `hsl()`, and `hsla()` functional forms.
//! - The `transparent` keyword.
//!
//! Lynx does **not** support `currentColor`, `hwb()`, `lab()`,
//! `oklch()`, the `color()` function, or any of the wide-gamut
//! color spaces from CSS Color Module Level 4. These variants are
//! intentionally absent so writing them in Rust is a compile error.

use core::fmt;

use crate::to_css::{write_number, ToCss};

use super::Angle;

mod named;

pub use named::NamedColor;

/// A CSS `<color>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Color {
    /// A CSS named color (`red`, `dodgerblue`, …).
    Named(NamedColor),
    /// `transparent` — the fully transparent black sentinel.
    Transparent,
    /// `rgb()` / `rgba()` form. Alpha is `0.0..=1.0`; `1.0` serializes
    /// as `rgb(...)`, any other alpha as `rgba(...)`.
    Rgba(u8, u8, u8, f32),
    /// `hsl()` / `hsla()` form. Saturation and lightness are
    /// percentages (`0.0..=100.0`); alpha is `0.0..=1.0`.
    Hsla {
        /// Hue, accepting any `<angle>` Lynx supports.
        h: Angle,
        /// Saturation as a percentage (`0.0..=100.0`).
        s: f32,
        /// Lightness as a percentage (`0.0..=100.0`).
        l: f32,
        /// Alpha (`0.0..=1.0`).
        a: f32,
    },
}

impl Color {
    /// Construct from a 24-bit `0xRRGGBB` packed integer. Alpha
    /// defaults to `1.0` (fully opaque).
    pub const fn hex(rgb: u32) -> Self {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        Self::Rgba(r, g, b, 1.0)
    }

    /// Construct from a 32-bit `0xRRGGBBAA` packed integer.
    pub fn hex_alpha(rgba: u32) -> Self {
        let r = ((rgba >> 24) & 0xFF) as u8;
        let g = ((rgba >> 16) & 0xFF) as u8;
        let b = ((rgba >> 8) & 0xFF) as u8;
        let a = (rgba & 0xFF) as u8;
        Self::Rgba(r, g, b, a as f32 / 255.0)
    }

    /// Construct from individual 8-bit RGB channels, fully opaque.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Rgba(r, g, b, 1.0)
    }

    /// Construct from individual 8-bit RGB channels with a custom
    /// alpha in the range `0.0..=1.0`.
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self::Rgba(r, g, b, a)
    }

    /// Construct from HSL components. Alpha defaults to `1.0`.
    pub const fn hsl(h_deg: f32, s_percent: f32, l_percent: f32) -> Self {
        Self::Hsla {
            h: Angle::Deg(h_deg),
            s: s_percent,
            l: l_percent,
            a: 1.0,
        }
    }

    /// Construct from HSL components with explicit alpha.
    pub const fn hsla(h_deg: f32, s_percent: f32, l_percent: f32, a: f32) -> Self {
        Self::Hsla {
            h: Angle::Deg(h_deg),
            s: s_percent,
            l: l_percent,
            a,
        }
    }
}

impl From<NamedColor> for Color {
    fn from(n: NamedColor) -> Self {
        Self::Named(n)
    }
}

impl ToCss for Color {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Color::Named(n) => dest.write_str(n.name()),
            Color::Transparent => dest.write_str("transparent"),
            Color::Rgba(r, g, b, a) => {
                if (*a - 1.0).abs() < f32::EPSILON {
                    write!(dest, "rgb({r}, {g}, {b})")
                } else {
                    write!(dest, "rgba({r}, {g}, {b}, ")?;
                    write_number(dest, *a)?;
                    dest.write_char(')')
                }
            }
            Color::Hsla { h, s, l, a } => {
                if (*a - 1.0).abs() < f32::EPSILON {
                    dest.write_str("hsl(")?;
                    h.to_css(dest)?;
                    dest.write_str(", ")?;
                    write_number(dest, *s)?;
                    dest.write_str("%, ")?;
                    write_number(dest, *l)?;
                    dest.write_str("%)")
                } else {
                    dest.write_str("hsla(")?;
                    h.to_css(dest)?;
                    dest.write_str(", ")?;
                    write_number(dest, *s)?;
                    dest.write_str("%, ")?;
                    write_number(dest, *l)?;
                    dest.write_str("%, ")?;
                    write_number(dest, *a)?;
                    dest.write_char(')')
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_color_serializes_to_keyword() {
        assert_eq!(Color::Named(NamedColor::Red).to_css_string(), "red");
        assert_eq!(
            Color::Named(NamedColor::DodgerBlue).to_css_string(),
            "dodgerblue"
        );
    }

    #[test]
    fn transparent_keyword() {
        assert_eq!(Color::Transparent.to_css_string(), "transparent");
    }

    #[test]
    fn hex_constructor() {
        // 0x1A1A2E → rgb(26, 26, 46)
        assert_eq!(Color::hex(0x1A1A2E).to_css_string(), "rgb(26, 26, 46)");
    }

    #[test]
    fn hex_alpha_constructor() {
        let c = Color::hex_alpha(0xFF000080);
        // 0x80 / 255 ≈ 0.5019608 — alpha drops it to rgba.
        let s = c.to_css_string();
        assert!(s.starts_with("rgba(255, 0, 0,"), "got {s}");
    }

    #[test]
    fn rgb_with_opaque_alpha_drops_rgba() {
        assert_eq!(Color::rgba(1, 2, 3, 1.0).to_css_string(), "rgb(1, 2, 3)");
    }

    #[test]
    fn rgb_with_partial_alpha_uses_rgba() {
        assert_eq!(
            Color::rgba(1, 2, 3, 0.5).to_css_string(),
            "rgba(1, 2, 3, 0.5)"
        );
    }

    #[test]
    fn hsl_opaque() {
        assert_eq!(
            Color::hsl(120.0, 50.0, 25.0).to_css_string(),
            "hsl(120deg, 50%, 25%)"
        );
    }

    #[test]
    fn hsla_partial() {
        assert_eq!(
            Color::hsla(0.0, 100.0, 50.0, 0.25).to_css_string(),
            "hsla(0deg, 100%, 50%, 0.25)"
        );
    }

    #[test]
    fn from_named_color() {
        let c: Color = NamedColor::Black.into();
        assert_eq!(c.to_css_string(), "black");
    }
}
