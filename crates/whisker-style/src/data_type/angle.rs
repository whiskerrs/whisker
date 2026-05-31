//! `<angle>` — a rotational quantity.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/angle.html>
//!
//! Lynx supports three of the four CSS angle units:
//!
//! | Variant | CSS | Notes |
//! |---|---|---|
//! | [`Angle::Deg`]  | `deg`  | 360 degrees per full turn |
//! | [`Angle::Rad`]  | `rad`  | 2π radians per full turn |
//! | [`Angle::Turn`] | `turn` | One full revolution |
//!
//! **`grad` is not supported by Lynx** and is intentionally absent.

use core::fmt;

use crate::to_css::{write_number, ToCss};

/// A CSS `<angle>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Angle {
    /// Degrees (`deg`). 360 per full turn.
    Deg(f32),
    /// Radians (`rad`). 2π per full turn.
    Rad(f32),
    /// Turns (`turn`). One full revolution.
    Turn(f32),
}

impl Angle {
    /// Convenience zero-degree constant.
    pub const ZERO: Self = Self::Deg(0.0);
}

impl ToCss for Angle {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        let (v, unit) = match *self {
            Angle::Deg(v) => (v, "deg"),
            Angle::Rad(v) => (v, "rad"),
            Angle::Turn(v) => (v, "turn"),
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
        assert_eq!(Angle::Deg(45.0).to_css_string(), "45deg");
        assert_eq!(Angle::Rad(1.5708).to_css_string(), "1.5708rad");
        assert_eq!(Angle::Turn(0.25).to_css_string(), "0.25turn");
    }

    #[test]
    fn zero_constant() {
        assert_eq!(Angle::ZERO.to_css_string(), "0deg");
    }

    #[test]
    fn negative_angles() {
        assert_eq!(Angle::Deg(-90.0).to_css_string(), "-90deg");
    }
}
