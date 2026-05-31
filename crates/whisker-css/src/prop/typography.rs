//! Typography properties: font, letter spacing, line height.

use crate::css::Css;
use crate::data_type::{CssString, Length, LengthPercentage};
use crate::keyword::{FontStyle, FontVariant, FontWeight};
use crate::value::LineHeight;

impl Css {
    /// Sets `font-family`. Pass a single family name; for multiple
    /// families, call this method once per family or use the
    /// [`Css::raw`] escape hatch with a comma-separated list.
    /// <https://lynxjs.org/api/css/properties/font-family>
    pub fn font_family(self, v: impl Into<String>) -> Self {
        // Lynx accepts either bare identifiers or quoted strings; quoting
        // unconditionally is always safe.
        self.push("font-family", CssString::new(v))
    }

    /// Sets `font-size`. Lynx default: `14px`.
    /// <https://lynxjs.org/api/css/properties/font-size>
    pub fn font_size(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("font-size", v.into())
    }

    /// Sets `font-style`. Lynx default: `normal`.
    /// <https://lynxjs.org/api/css/properties/font-style>
    pub fn font_style(self, v: FontStyle) -> Self {
        self.push("font-style", v)
    }

    /// Sets `font-weight`. Lynx default: `normal`. `bolder`/`lighter`
    /// are not supported.
    /// <https://lynxjs.org/api/css/properties/font-weight>
    pub fn font_weight(self, v: FontWeight) -> Self {
        self.push("font-weight", v)
    }

    /// Sets `font-variant`.
    /// <https://lynxjs.org/api/css/properties/font-variant>
    pub fn font_variant(self, v: FontVariant) -> Self {
        self.push("font-variant", v)
    }

    /// Sets `letter-spacing`. Accepts `<length>`.
    /// <https://lynxjs.org/api/css/properties/letter-spacing>
    pub fn letter_spacing(self, v: Length) -> Self {
        self.push("letter-spacing", v)
    }

    /// Sets `line-height`. Lynx default: `normal`.
    /// <https://lynxjs.org/api/css/properties/line-height>
    pub fn line_height(self, v: impl Into<LineHeight>) -> Self {
        self.push("line-height", v.into())
    }
}

#[cfg(test)]
mod tests {
    use crate::ext::*;
    use crate::keyword::*;
    use crate::value::LineHeight;
    use crate::Css;

    #[test]
    fn font_family_quotes_the_value() {
        let s = Css::new().font_family("Helvetica Neue");
        assert_eq!(s.to_string(), "font-family: \"Helvetica Neue\";");
    }

    #[test]
    fn font_size_length_or_percentage() {
        assert_eq!(Css::new().font_size(px(16)).to_string(), "font-size: 16px;");
        assert_eq!(
            Css::new().font_size(percent(120)).to_string(),
            "font-size: 120%;"
        );
    }

    #[test]
    fn font_style_keywords() {
        assert_eq!(
            Css::new().font_style(FontStyle::Italic).to_string(),
            "font-style: italic;"
        );
    }

    #[test]
    fn font_weight_keyword_and_numeric() {
        assert_eq!(
            Css::new().font_weight(FontWeight::Bold).to_string(),
            "font-weight: bold;"
        );
        assert_eq!(
            Css::new().font_weight(FontWeight::Numeric(600)).to_string(),
            "font-weight: 600;"
        );
    }

    #[test]
    fn font_variant_small_caps() {
        assert_eq!(
            Css::new().font_variant(FontVariant::SmallCaps).to_string(),
            "font-variant: small-caps;"
        );
    }

    #[test]
    fn letter_spacing_length() {
        let s = Css::new().letter_spacing(px(2));
        assert_eq!(s.to_string(), "letter-spacing: 2px;");
    }

    #[test]
    fn line_height_variants() {
        assert_eq!(
            Css::new().line_height(LineHeight::Normal).to_string(),
            "line-height: normal;"
        );
        assert_eq!(
            Css::new().line_height(1.5_f32).to_string(),
            "line-height: 1.5;"
        );
        assert_eq!(
            Css::new().line_height(px(24)).to_string(),
            "line-height: 24px;"
        );
        assert_eq!(
            Css::new().line_height(percent(150)).to_string(),
            "line-height: 150%;"
        );
    }
}
