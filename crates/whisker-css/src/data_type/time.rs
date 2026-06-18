//! `<time>` — a duration.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/time.html>
//!
//! Lynx accepts two CSS time units:
//!
//! | Variant | CSS | Notes |
//! |---|---|---|
//! | [`Time::S`]  | `s`  | Seconds, may be negative |
//! | [`Time::Ms`] | `ms` | Milliseconds, may be negative |
//!
//! Unlike `<length>`, **a bare `0` is not a valid `<time>`** in
//! Lynx — you must write `0s` or `0ms`. This is enforced by the
//! shape of the enum: there is no zero variant.

use core::fmt;

use crate::to_css::{ToCss, write_number};

/// A CSS `<time>` value.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Time {
    /// Seconds (`s`).
    S(f32),
    /// Milliseconds (`ms`).
    Ms(f32),
}

impl ToCss for Time {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        let (v, unit) = match *self {
            Time::S(v) => (v, "s"),
            Time::Ms(v) => (v, "ms"),
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
        assert_eq!(Time::S(1.5).to_css_string(), "1.5s");
        assert_eq!(Time::Ms(300.0).to_css_string(), "300ms");
    }

    #[test]
    fn negative_time() {
        // Negative delays are valid in `transition-delay`.
        assert_eq!(Time::Ms(-100.0).to_css_string(), "-100ms");
    }

    #[test]
    fn zero_keeps_unit() {
        // Crucial: a bare `0` is invalid; we always emit the unit.
        assert_eq!(Time::S(0.0).to_css_string(), "0s");
        assert_eq!(Time::Ms(0.0).to_css_string(), "0ms");
    }
}
