//! `<percentage>` — a fractional value relative to some reference.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/percentage.html>
//!
//! Lynx requires the `%` suffix unconditionally — a bare `0` is
//! **not** a valid percentage even though it is a valid
//! [`Length`](super::Length). The reference quantity depends on the
//! property: `width: 50%` resolves against the containing block, but
//! `transform: translate(50%, 0)` resolves against the element's own
//! box.

use core::fmt;

use crate::to_css::{write_number, ToCss};

/// A CSS `<percentage>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Percentage(pub f32);

impl Percentage {
    /// Construct from the unitless ratio multiplied by 100.
    ///
    /// `Percentage::new(50.0)` produces `50%`. The constructor takes
    /// the percentage *number*, not a `0.0..=1.0` ratio, to match
    /// what is written in CSS.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    /// Underlying value (the number before `%`).
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl From<f32> for Percentage {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

impl From<i32> for Percentage {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl ToCss for Percentage {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        write_number(dest, self.0)?;
        dest.write_char('%')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_percent() {
        assert_eq!(Percentage(50.0).to_css_string(), "50%");
        assert_eq!(Percentage(100.0).to_css_string(), "100%");
    }

    #[test]
    fn fractional_percent() {
        assert_eq!(Percentage(33.3).to_css_string(), "33.3%");
    }

    #[test]
    fn zero_keeps_percent_sign() {
        // Lynx does not allow bare `0` as a percentage.
        assert_eq!(Percentage(0.0).to_css_string(), "0%");
    }

    #[test]
    fn from_impls() {
        assert_eq!(Percentage::from(25), Percentage(25.0));
        assert_eq!(Percentage::from(12.5_f32), Percentage(12.5));
    }
}
