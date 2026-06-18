//! `<number>` — a real number.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/number.html>
//!
//! Lynx accepts integers, decimals, and scientific notation (`10e3`,
//! `-1.2`, `0.5`). The literals `12.` (trailing dot) and `.5e` are
//! rejected by the Lynx parser; constructing [`Number`] from `f32`
//! is unrestricted because the value is re-serialized through
//! [`write_number`](crate::to_css::write_number), which always
//! emits a well-formed literal.

use core::fmt;

use crate::to_css::{ToCss, write_number};

/// A CSS `<number>` value.
///
/// Wraps `f32` so call sites carry intent (`Number(1.0)` vs. a bare
/// `1.0` which could be a percentage, an opacity, or a flex factor).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Number(pub f32);

impl Number {
    /// Construct from a primitive `f32`.
    pub const fn new(v: f32) -> Self {
        Self(v)
    }

    /// Underlying value.
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl From<f32> for Number {
    fn from(v: f32) -> Self {
        Self(v)
    }
}

impl From<i32> for Number {
    fn from(v: i32) -> Self {
        Self(v as f32)
    }
}

impl ToCss for Number {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        write_number(dest, self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_serializes_without_decimal() {
        assert_eq!(Number(1.0).to_css_string(), "1");
        assert_eq!(Number(-3.0).to_css_string(), "-3");
        assert_eq!(Number(0.0).to_css_string(), "0");
    }

    #[test]
    fn fractional_serializes_with_decimal() {
        assert_eq!(Number(0.5).to_css_string(), "0.5");
        assert_eq!(Number(-1.25).to_css_string(), "-1.25");
    }

    #[test]
    fn from_impls() {
        assert_eq!(Number::from(2_i32), Number(2.0));
        assert_eq!(Number::from(2.5_f32), Number(2.5));
    }

    #[test]
    fn accessors() {
        let n = Number::new(7.0);
        assert_eq!(n.value(), 7.0);
    }
}
