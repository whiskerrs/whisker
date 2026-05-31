//! The `ToCss` trait — formats a value to its canonical CSS form.

use core::fmt;

/// Format a value as the CSS source text Lynx will parse.
///
/// Implementors must write a string that, when fed back into Lynx's
/// CSS parser for the property accepting the value, produces an
/// equivalent computed value. Whitespace, casing, and unit choice
/// follow the canonical CSS form documented at
/// <https://lynxjs.org/api/css>.
///
/// A blanket [`fmt::Display`] is **not** provided automatically so
/// implementors stay explicit about which surface (CSS-text vs.
/// debug) they're writing.
pub trait ToCss {
    /// Write the CSS representation of `self` into `dest`.
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result;

    /// Convenience: allocate a fresh [`String`] and write into it.
    fn to_css_string(&self) -> String {
        let mut s = String::new();
        // Writing into a `String` cannot fail in practice (no I/O).
        let _ = self.to_css(&mut s);
        s
    }
}

impl<T: ToCss + ?Sized> ToCss for &T {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        (**self).to_css(dest)
    }
}

/// Render an `f32` without a trailing `.0` for integer values so the
/// emitted CSS stays compact.
pub(crate) fn write_number(dest: &mut dyn fmt::Write, n: f32) -> fmt::Result {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e16 {
        write!(dest, "{}", n as i64)
    } else {
        write!(dest, "{n}")
    }
}

/// Convenience: render an `f32` to a fresh [`String`] using the same
/// rules as [`write_number`].
pub(crate) fn number_to_string(n: f32) -> String {
    let mut s = String::new();
    let _ = write_number(&mut s, n);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny helper type to exercise the blanket [`ToCss`] impl on
    /// references.
    struct Token(&'static str);
    impl ToCss for Token {
        fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
            dest.write_str(self.0)
        }
    }

    #[test]
    fn write_number_drops_decimal_for_integers() {
        let mut buf = String::new();
        write_number(&mut buf, 1.0).unwrap();
        assert_eq!(buf, "1");
        buf.clear();
        write_number(&mut buf, -3.0).unwrap();
        assert_eq!(buf, "-3");
        buf.clear();
        write_number(&mut buf, 0.0).unwrap();
        assert_eq!(buf, "0");
    }

    #[test]
    fn write_number_keeps_decimal_for_fractions() {
        let mut buf = String::new();
        write_number(&mut buf, 0.5).unwrap();
        assert_eq!(buf, "0.5");
        buf.clear();
        write_number(&mut buf, -1.25).unwrap();
        assert_eq!(buf, "-1.25");
    }

    #[test]
    fn write_number_handles_non_finite_safely() {
        let mut buf = String::new();
        // Non-finite values fall through to `{n}` formatting; the
        // helper just needs to not panic and to keep something on
        // the buffer.
        let _ = write_number(&mut buf, f32::NAN);
        let _ = write_number(&mut buf, f32::INFINITY);
        // Smoke check — output is non-empty.
        assert!(!buf.is_empty());
    }

    #[test]
    fn write_number_handles_huge_floats() {
        let mut buf = String::new();
        // Values past the safe-integer threshold fall through to
        // floating-point formatting.
        write_number(&mut buf, 1e20).unwrap();
        assert!(!buf.is_empty());
    }

    #[test]
    fn number_to_string_matches_write_number() {
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(0.25), "0.25");
        assert_eq!(number_to_string(-2.5), "-2.5");
    }

    #[test]
    fn to_css_blanket_reference_impl() {
        // Verify `&T: ToCss` where `T: ToCss` works.
        let t = Token("ident");
        let r: &Token = &t;
        let s = r.to_css_string();
        assert_eq!(s, "ident");
    }

    #[test]
    fn to_css_string_uses_the_same_path() {
        let t = Token("abc");
        assert_eq!(t.to_css_string(), "abc");
    }
}
