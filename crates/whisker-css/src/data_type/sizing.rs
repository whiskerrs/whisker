//! `<fit-content>` and `<max-content>` — track-sizing keywords used
//! by `width`, `height`, `min-width`, `min-height`, `max-width`, and
//! `max-height`.
//!
//! Lynx references:
//! - <https://lynxjs.org/api/css/data-type/fit-content.html>
//! - <https://lynxjs.org/api/css/data-type/max-content.html>

use core::fmt;

use crate::to_css::ToCss;

use super::LengthPercentage;

/// `<fit-content>` — sizes the box to the content with an optional
/// upper bound.
///
/// `FitContent(None)` serializes to the bare `fit-content` keyword;
/// `FitContent(Some(limit))` serializes to `fit-content(<limit>)`.
#[derive(Clone, Debug, PartialEq)]
pub struct FitContent(pub Option<LengthPercentage>);

impl FitContent {
    /// `fit-content` without an upper bound.
    pub const fn keyword() -> Self {
        Self(None)
    }

    /// `fit-content(<limit>)`.
    pub fn with_limit(limit: impl Into<LengthPercentage>) -> Self {
        Self(Some(limit.into()))
    }
}

impl ToCss for FitContent {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str("fit-content")?;
        if let Some(limit) = &self.0 {
            dest.write_char('(')?;
            limit.to_css(dest)?;
            dest.write_char(')')?;
        }
        Ok(())
    }
}

/// `<max-content>` — sizes the box to its maximum intrinsic content
/// size.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct MaxContent;

impl ToCss for MaxContent {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str("max-content")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_type::Length;

    #[test]
    fn fit_content_keyword() {
        assert_eq!(FitContent::keyword().to_css_string(), "fit-content");
    }

    #[test]
    fn fit_content_with_limit() {
        let fc = FitContent::with_limit(Length::Px(200.0));
        assert_eq!(fc.to_css_string(), "fit-content(200px)");
    }

    #[test]
    fn max_content_keyword() {
        assert_eq!(MaxContent.to_css_string(), "max-content");
    }
}
