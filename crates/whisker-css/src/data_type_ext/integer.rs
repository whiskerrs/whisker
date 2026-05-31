//! `<integer>` — a signed whole number.
//!
//! Lynx does not document `<integer>` as a standalone data type
//! (it folds the concept into `<number>` at
//! <https://lynxjs.org/api/css/data-type/number.html>), but several
//! properties — `z-index`, `order`, `animation-iteration-count` —
//! reject fractional values. Exposing an `Integer` newtype at the
//! property surface makes those constraints visible in the Rust
//! type system.

use core::fmt;

use crate::to_css::ToCss;

/// A CSS `<integer>` value.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Integer(pub i32);

impl Integer {
    /// Construct from a primitive `i32`.
    pub const fn new(v: i32) -> Self {
        Self(v)
    }

    /// Underlying value.
    pub const fn value(self) -> i32 {
        self.0
    }
}

impl From<i32> for Integer {
    fn from(v: i32) -> Self {
        Self(v)
    }
}

impl ToCss for Integer {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        write!(dest, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_and_zero() {
        assert_eq!(Integer(0).to_css_string(), "0");
        assert_eq!(Integer(42).to_css_string(), "42");
    }

    #[test]
    fn negative() {
        assert_eq!(Integer(-7).to_css_string(), "-7");
    }

    #[test]
    fn accessors_and_conversion() {
        let i: Integer = 5.into();
        assert_eq!(i.value(), 5);
        assert_eq!(Integer::new(9).value(), 9);
    }
}
