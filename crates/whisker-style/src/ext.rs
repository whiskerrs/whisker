//! Numeric-literal extension traits and free constructors.
//!
//! These keep call sites concise:
//!
//! ```
//! use whisker_style::ext::*;
//!
//! let _ = px(12);
//! let _ = 12.px();
//! let _ = 0.5.rem();
//! let _ = 100.percent();
//! ```

use crate::data_type::{Angle, Length, Percentage, Time};

/// Internal: anything that can be widened to `f32` for value
/// construction.
///
/// `i32 → f32` is not in `std` as `From` because it can lose
/// precision; for CSS lengths the precision loss is irrelevant, so
/// the trait is provided locally.
pub trait IntoF32: Copy {
    /// Convert to `f32`.
    fn into_f32(self) -> f32;
}

impl IntoF32 for f32 {
    fn into_f32(self) -> f32 {
        self
    }
}

impl IntoF32 for i32 {
    fn into_f32(self) -> f32 {
        self as f32
    }
}

impl IntoF32 for u32 {
    fn into_f32(self) -> f32 {
        self as f32
    }
}

// ---------- Length ----------

/// Construct a [`Length::Px`] (logical pixels).
pub fn px(v: impl IntoF32) -> Length {
    Length::Px(v.into_f32())
}

/// Construct a [`Length::Rpx`] (Lynx responsive pixel; 750rpx = device width).
pub fn rpx(v: impl IntoF32) -> Length {
    Length::Rpx(v.into_f32())
}

/// Construct a [`Length::Ppx`] (physical pixel).
pub fn ppx(v: impl IntoF32) -> Length {
    Length::Ppx(v.into_f32())
}

/// Construct a [`Length::Em`].
pub fn em(v: impl IntoF32) -> Length {
    Length::Em(v.into_f32())
}

/// Construct a [`Length::Rem`].
pub fn rem(v: impl IntoF32) -> Length {
    Length::Rem(v.into_f32())
}

/// Construct a [`Length::Vh`].
pub fn vh(v: impl IntoF32) -> Length {
    Length::Vh(v.into_f32())
}

/// Construct a [`Length::Vw`].
pub fn vw(v: impl IntoF32) -> Length {
    Length::Vw(v.into_f32())
}

/// The unit-less zero — the only length CSS allows without a unit.
pub const ZERO: Length = Length::Zero;

/// Method-style length constructors for primitive numbers.
///
/// Implemented for both `f32` and `i32`. `i32` widens silently to
/// `f32`, so `8.px()` and `8.0.px()` are interchangeable.
pub trait LengthExt: Copy {
    /// `<self>px`.
    fn px(self) -> Length;
    /// `<self>rpx`.
    fn rpx(self) -> Length;
    /// `<self>ppx`.
    fn ppx(self) -> Length;
    /// `<self>em`.
    fn em(self) -> Length;
    /// `<self>rem`.
    fn rem(self) -> Length;
    /// `<self>vh`.
    fn vh(self) -> Length;
    /// `<self>vw`.
    fn vw(self) -> Length;
}

impl<T: IntoF32> LengthExt for T {
    fn px(self) -> Length {
        Length::Px(self.into_f32())
    }
    fn rpx(self) -> Length {
        Length::Rpx(self.into_f32())
    }
    fn ppx(self) -> Length {
        Length::Ppx(self.into_f32())
    }
    fn em(self) -> Length {
        Length::Em(self.into_f32())
    }
    fn rem(self) -> Length {
        Length::Rem(self.into_f32())
    }
    fn vh(self) -> Length {
        Length::Vh(self.into_f32())
    }
    fn vw(self) -> Length {
        Length::Vw(self.into_f32())
    }
}

// ---------- Percentage ----------

/// Construct a [`Percentage`] from `n` (so `percent(50)` == `50%`).
pub fn percent(v: impl IntoF32) -> Percentage {
    Percentage(v.into_f32())
}

/// Method-style percentage constructor.
pub trait PercentExt: Copy {
    /// `<self>%`.
    fn percent(self) -> Percentage;
}

impl<T: IntoF32> PercentExt for T {
    fn percent(self) -> Percentage {
        Percentage(self.into_f32())
    }
}

// ---------- Angle ----------

/// Construct an [`Angle::Deg`].
pub fn deg(v: impl IntoF32) -> Angle {
    Angle::Deg(v.into_f32())
}

/// Construct an [`Angle::Rad`].
pub fn rad(v: impl IntoF32) -> Angle {
    Angle::Rad(v.into_f32())
}

/// Construct an [`Angle::Turn`].
pub fn turn(v: impl IntoF32) -> Angle {
    Angle::Turn(v.into_f32())
}

/// Method-style angle constructors.
pub trait AngleExt: Copy {
    /// `<self>deg`.
    fn deg(self) -> Angle;
    /// `<self>rad`.
    fn rad(self) -> Angle;
    /// `<self>turn`.
    fn turn(self) -> Angle;
}

impl<T: IntoF32> AngleExt for T {
    fn deg(self) -> Angle {
        Angle::Deg(self.into_f32())
    }
    fn rad(self) -> Angle {
        Angle::Rad(self.into_f32())
    }
    fn turn(self) -> Angle {
        Angle::Turn(self.into_f32())
    }
}

// ---------- Time ----------

/// Construct a [`Time::S`] (seconds).
pub fn s(v: impl IntoF32) -> Time {
    Time::S(v.into_f32())
}

/// Construct a [`Time::Ms`] (milliseconds).
pub fn ms(v: impl IntoF32) -> Time {
    Time::Ms(v.into_f32())
}

/// Method-style time constructors.
pub trait TimeExt: Copy {
    /// `<self>s`.
    fn s(self) -> Time;
    /// `<self>ms`.
    fn ms(self) -> Time;
}

impl<T: IntoF32> TimeExt for T {
    fn s(self) -> Time {
        Time::S(self.into_f32())
    }
    fn ms(self) -> Time {
        Time::Ms(self.into_f32())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_css::ToCss;

    #[test]
    fn length_free_fns() {
        assert_eq!(px(8).to_css_string(), "8px");
        assert_eq!(rpx(750).to_css_string(), "750rpx");
        assert_eq!(ppx(2).to_css_string(), "2ppx");
        assert_eq!(em(1.5).to_css_string(), "1.5em");
        assert_eq!(rem(2).to_css_string(), "2rem");
        assert_eq!(vh(50).to_css_string(), "50vh");
        assert_eq!(vw(100).to_css_string(), "100vw");
    }

    #[test]
    fn length_methods_on_i32() {
        assert_eq!(8.px().to_css_string(), "8px");
        assert_eq!(750.rpx().to_css_string(), "750rpx");
        assert_eq!(2.ppx().to_css_string(), "2ppx");
        assert_eq!(1.em().to_css_string(), "1em");
        assert_eq!(2.rem().to_css_string(), "2rem");
        assert_eq!(50.vh().to_css_string(), "50vh");
        assert_eq!(100.vw().to_css_string(), "100vw");
    }

    #[test]
    fn length_methods_on_f32() {
        assert_eq!(0.5_f32.px().to_css_string(), "0.5px");
        assert_eq!(1.5_f32.em().to_css_string(), "1.5em");
        assert_eq!(0.5_f32.rem().to_css_string(), "0.5rem");
        assert_eq!(0.5_f32.vh().to_css_string(), "0.5vh");
        assert_eq!(0.5_f32.vw().to_css_string(), "0.5vw");
        assert_eq!(0.5_f32.rpx().to_css_string(), "0.5rpx");
        assert_eq!(0.5_f32.ppx().to_css_string(), "0.5ppx");
    }

    #[test]
    fn percent_helpers() {
        assert_eq!(percent(50).to_css_string(), "50%");
        assert_eq!(50.percent().to_css_string(), "50%");
        assert_eq!(33.3_f32.percent().to_css_string(), "33.3%");
    }

    #[test]
    fn angle_helpers() {
        assert_eq!(deg(90).to_css_string(), "90deg");
        assert_eq!(rad(1).to_css_string(), "1rad");
        assert_eq!(turn(1).to_css_string(), "1turn");
        assert_eq!(45.deg().to_css_string(), "45deg");
        assert_eq!(2.rad().to_css_string(), "2rad");
        assert_eq!(0.5_f32.turn().to_css_string(), "0.5turn");
        assert_eq!(0.5_f32.deg().to_css_string(), "0.5deg");
        assert_eq!(0.5_f32.rad().to_css_string(), "0.5rad");
    }

    #[test]
    fn time_helpers() {
        assert_eq!(s(1).to_css_string(), "1s");
        assert_eq!(ms(300).to_css_string(), "300ms");
        assert_eq!(1.s().to_css_string(), "1s");
        assert_eq!(300.ms().to_css_string(), "300ms");
        assert_eq!(0.5_f32.s().to_css_string(), "0.5s");
        assert_eq!(1.5_f32.ms().to_css_string(), "1.5ms");
    }

    #[test]
    fn into_f32_for_u32() {
        assert_eq!(IntoF32::into_f32(7_u32), 7.0);
        assert_eq!(7_u32.px().to_css_string(), "7px");
    }

    #[test]
    fn zero_constant() {
        assert_eq!(ZERO.to_css_string(), "0");
    }
}
