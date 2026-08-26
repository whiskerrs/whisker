//! Typography properties: font, letter spacing, line height.

use crate::css::Css;
use crate::data_type::{CssString, Length, LengthPercentage};
use crate::keyword::{FontStyle, FontVariant, FontWeight};
use crate::to_css::ToCss;
use crate::value::LineHeight;
use whisker_style::{FontFeatureValue, FontOpticalSizingValue, FontVariationValue};

impl Css {
    /// Sets `font-family`. Pass a single family name; for multiple
    /// families, call this method once per family or use the
    /// [`Css::raw`] escape hatch with a comma-separated list.
    /// <https://lynxjs.org/api/css/properties/font-family>
    pub fn font_family(self, v: impl Into<String>) -> Self {
        // Lynx accepts either bare identifiers or quoted strings; quoting
        // unconditionally is always safe.
        let name = v.into();
        self.push_semantic(
            crate::StyleProperty::FontFamily,
            whisker_style::StyleValue::FontFamily(whisker_style::FontFamilyValue::Named(
                name.clone(),
            )),
            CssString::new(name).to_css_string(),
        )
    }

    /// Sets `font-feature-settings` from validated OpenType tags. An empty
    /// vector serializes as `normal`.
    /// <https://lynxjs.org/api/css/properties/font-feature-settings>
    pub fn font_feature_settings(self, values: Vec<FontFeatureValue>) -> Self {
        let css = if values.is_empty() {
            "normal".to_string()
        } else {
            values
                .iter()
                .map(|setting| format!("'{}' {}", tag_text(setting.tag.get()), setting.value))
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.push_semantic(
            crate::StyleProperty::FontFeatureSettings,
            whisker_style::StyleValue::FontFeatures(values),
            css,
        )
    }

    /// Sets `font-variation-settings` from validated OpenType axis tags. An
    /// empty vector serializes as `normal`.
    /// <https://lynxjs.org/api/css/properties/font-variation-settings>
    pub fn font_variation_settings(self, values: Vec<FontVariationValue>) -> Self {
        let css = if values.is_empty() {
            "normal".to_string()
        } else {
            values
                .iter()
                .map(|setting| format!("'{}' {}", tag_text(setting.tag.get()), setting.value.get()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        self.push_semantic(
            crate::StyleProperty::FontVariationSettings,
            whisker_style::StyleValue::FontVariations(values),
            css,
        )
    }

    /// Sets Lynx `font-optical-sizing` (`none` is Lynx's initial value).
    /// <https://lynxjs.org/api/css/properties/font-optical-sizing>
    pub fn font_optical_sizing(self, value: FontOpticalSizingValue) -> Self {
        self.push_semantic(
            crate::StyleProperty::FontOpticalSizing,
            whisker_style::StyleValue::FontOpticalSizing(value),
            match value {
                FontOpticalSizingValue::Auto => "auto",
                FontOpticalSizingValue::None => "none",
            },
        )
    }

    /// Sets `font-size`. Lynx default: `14px`.
    /// <https://lynxjs.org/api/css/properties/font-size>
    pub fn font_size(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::FontSize, v.into())
    }

    /// Sets `font-style`. Lynx default: `normal`.
    /// <https://lynxjs.org/api/css/properties/font-style>
    pub fn font_style(self, v: FontStyle) -> Self {
        self.push_typed(crate::StyleProperty::FontStyle, v)
    }

    /// Sets `font-weight`. Lynx default: `normal`. `bolder`/`lighter`
    /// are not supported.
    /// <https://lynxjs.org/api/css/properties/font-weight>
    pub fn font_weight(self, v: FontWeight) -> Self {
        self.push_typed(crate::StyleProperty::FontWeight, v)
    }

    /// Sets `font-variant`.
    /// <https://lynxjs.org/api/css/properties/font-variant>
    pub fn font_variant(self, v: FontVariant) -> Self {
        self.push(crate::StyleProperty::FontVariant, v)
    }

    /// Sets `letter-spacing`. Accepts `<length>`.
    /// <https://lynxjs.org/api/css/properties/letter-spacing>
    pub fn letter_spacing(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::LetterSpacing, v)
    }

    /// Sets `line-height`. Lynx default: `normal`.
    /// <https://lynxjs.org/api/css/properties/line-height>
    pub fn line_height(self, v: impl Into<LineHeight>) -> Self {
        self.push_typed(crate::StyleProperty::LineHeight, v.into())
    }
}

fn tag_text(tag: [u8; 4]) -> String {
    String::from_utf8(tag.to_vec()).expect("OpenType tags are printable ASCII")
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::ext::*;
    use crate::keyword::*;
    use crate::value::LineHeight;
    use whisker_style::{
        FontFeatureValue, FontOpticalSizingValue, FontVariationValue, OpenTypeTagValue, StyleNumber,
    };

    #[test]
    fn font_family_quotes_the_value() {
        let s = Css::new().font_family("Helvetica Neue");
        assert_eq!(s.to_string(), "font-family: \"Helvetica Neue\";");
    }

    #[test]
    fn extended_font_settings_are_typed() {
        let kern = OpenTypeTagValue::new(*b"kern").unwrap();
        let weight = OpenTypeTagValue::new(*b"wght").unwrap();
        let css = Css::new()
            .font_feature_settings(vec![FontFeatureValue {
                tag: kern,
                value: 0,
            }])
            .font_variation_settings(vec![FontVariationValue {
                tag: weight,
                value: StyleNumber::new(650.0),
            }])
            .font_optical_sizing(FontOpticalSizingValue::Auto);
        assert_eq!(
            css.to_string(),
            "font-feature-settings: 'kern' 0; font-variation-settings: 'wght' 650; font-optical-sizing: auto;"
        );
        assert!(css.to_specified_style().is_ok());
        assert_eq!(
            Css::new()
                .font_feature_settings(Vec::new())
                .font_variation_settings(Vec::new())
                .to_string(),
            "font-feature-settings: normal; font-variation-settings: normal;"
        );
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
