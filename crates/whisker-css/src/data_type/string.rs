//! `<string>` — a quoted CSS string literal.
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/string.html>
//!
//! Used by properties such as `content`, `font-family` (when the
//! family name contains whitespace or punctuation), and the
//! `url("...")` function. The string is double-quoted on
//! serialization; backslashes and embedded double quotes are
//! escaped per CSS rules.

use core::fmt;

use crate::to_css::ToCss;

/// A CSS `<string>` value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CssString(pub String);

impl CssString {
    /// Construct from anything string-like.
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Borrow the inner `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CssString {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for CssString {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl ToCss for CssString {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_char('"')?;
        for ch in self.0.chars() {
            match ch {
                '"' => dest.write_str("\\\"")?,
                '\\' => dest.write_str("\\\\")?,
                '\n' => dest.write_str("\\A ")?,
                '\r' => dest.write_str("\\D ")?,
                c => dest.write_char(c)?,
            }
        }
        dest.write_char('"')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_is_quoted() {
        assert_eq!(CssString::new("hello").to_css_string(), "\"hello\"");
    }

    #[test]
    fn embedded_quote_is_escaped() {
        assert_eq!(
            CssString::new("a\"b").to_css_string(),
            "\"a\\\"b\""
        );
    }

    #[test]
    fn backslash_is_escaped() {
        assert_eq!(
            CssString::new("a\\b").to_css_string(),
            "\"a\\\\b\""
        );
    }

    #[test]
    fn newline_uses_css_escape() {
        assert_eq!(
            CssString::new("a\nb").to_css_string(),
            "\"a\\A b\""
        );
    }

    #[test]
    fn from_str_and_string() {
        let from_str: CssString = "x".into();
        let from_string: CssString = String::from("y").into();
        assert_eq!(from_str.to_css_string(), "\"x\"");
        assert_eq!(from_string.to_css_string(), "\"y\"");
        assert_eq!(from_str.as_str(), "x");
    }
}
