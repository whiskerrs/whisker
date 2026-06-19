//! `<length>` — a distance.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/length.html>
//!
//! Lynx supports a subset of CSS length units:
//!
//! | Variant | CSS | Reference |
//! |---|---|---|
//! | [`Length::Px`]  | `px`  | iOS points / Android dp |
//! | [`Length::Rpx`] | `rpx` | Lynx-specific: `750rpx` = device width |
//! | [`Length::Ppx`] | `ppx` | Physical pixels (device resolution) |
//! | [`Length::Em`]  | `em`  | The element's computed `font-size` |
//! | [`Length::Rem`] | `rem` | The root element's computed `font-size` |
//! | [`Length::Vh`]  | `vh`  | 1% of viewport height |
//! | [`Length::Vw`]  | `vw`  | 1% of viewport width |
//! | [`Length::Zero`]| `0`   | The unitless zero — only zero is allowed without a unit |
//!
//! The web units `cm`, `mm`, `in`, `pt`, `pc`, `ch`, `ex`, `lh`,
//! `rlh` are **not** part of Lynx and are intentionally absent from
//! this enum.

use core::fmt;

use crate::to_css::{ToCss, write_number};

/// A CSS `<length>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Length {
    /// Logical pixels (`px`). Maps to iOS points and Android dp.
    Px(f32),
    /// Lynx-specific responsive pixel (`rpx`). `750rpx` equals the
    /// device's screen width regardless of pixel density.
    Rpx(f32),
    /// Physical pixels (`ppx`). One device pixel.
    Ppx(f32),
    /// Em (`em`) — relative to the element's computed `font-size`.
    Em(f32),
    /// Root em (`rem`) — relative to the root element's
    /// computed `font-size`.
    Rem(f32),
    /// Viewport height (`vh`) — 1% of the viewport's height.
    Vh(f32),
    /// Viewport width (`vw`) — 1% of the viewport's width.
    Vw(f32),
    /// The unitless zero. CSS allows `0` (and only `0`) without a
    /// unit; this variant covers that case so a non-zero unit-less
    /// length is unrepresentable.
    Zero,
}

impl Length {
    /// Returns `true` when the length is exactly zero, regardless of
    /// the unit chosen at construction.
    pub fn is_zero(self) -> bool {
        match self {
            Length::Zero => true,
            Length::Px(v)
            | Length::Rpx(v)
            | Length::Ppx(v)
            | Length::Em(v)
            | Length::Rem(v)
            | Length::Vh(v)
            | Length::Vw(v) => v == 0.0,
        }
    }
}

impl ToCss for Length {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        let (v, unit) = match *self {
            Length::Zero => return dest.write_char('0'),
            Length::Px(v) => (v, "px"),
            Length::Rpx(v) => (v, "rpx"),
            Length::Ppx(v) => (v, "ppx"),
            Length::Em(v) => (v, "em"),
            Length::Rem(v) => (v, "rem"),
            Length::Vh(v) => (v, "vh"),
            Length::Vw(v) => (v, "vw"),
        };
        write_number(dest, v)?;
        dest.write_str(unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_unit_serializes() {
        assert_eq!(Length::Px(8.0).to_css_string(), "8px");
        assert_eq!(Length::Rpx(750.0).to_css_string(), "750rpx");
        assert_eq!(Length::Ppx(2.0).to_css_string(), "2ppx");
        assert_eq!(Length::Em(1.5).to_css_string(), "1.5em");
        assert_eq!(Length::Rem(1.0).to_css_string(), "1rem");
        assert_eq!(Length::Vh(50.0).to_css_string(), "50vh");
        assert_eq!(Length::Vw(100.0).to_css_string(), "100vw");
    }

    #[test]
    fn zero_serializes_unitless() {
        assert_eq!(Length::Zero.to_css_string(), "0");
    }

    #[test]
    fn fractional_values_keep_decimal() {
        assert_eq!(Length::Px(0.5).to_css_string(), "0.5px");
        assert_eq!(Length::Px(-1.25).to_css_string(), "-1.25px");
    }

    #[test]
    fn is_zero_detects_all_variants() {
        assert!(Length::Zero.is_zero());
        assert!(Length::Px(0.0).is_zero());
        assert!(Length::Vh(0.0).is_zero());
        assert!(!Length::Px(0.1).is_zero());
        assert!(!Length::Rpx(1.0).is_zero());
    }
}
