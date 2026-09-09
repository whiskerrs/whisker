use crate::StyleProperty;

/// Where a declaration has meaning inside a Text subtree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStyleScope {
    /// The outer Text box and its line layout.
    Paragraph,
    /// A logical span with no independent box.
    Run,
    /// An atomic View or Image with its own layout.
    Attachment,
}

impl StyleProperty {
    /// Classifies legal style placement independently of Host rendering support.
    pub const fn supports_text_scope(self, scope: TextStyleScope) -> bool {
        if !matches!(scope, TextStyleScope::Run) {
            return true;
        }
        matches!(
            self,
            Self::Color
                | Self::FontFamily
                | Self::FontSize
                | Self::FontWeight
                | Self::FontStyle
                | Self::FontFeatureSettings
                | Self::FontVariationSettings
                | Self::FontOpticalSizing
                | Self::FontVariant
                | Self::LetterSpacing
                | Self::TextDecoration
                | Self::TextDecorationLine
                | Self::TextDecorationColor
                | Self::TextDecorationStyle
                | Self::TextDecorationThickness
                | Self::TextShadow
                | Self::BackgroundColor
                | Self::BorderRadius
                | Self::BorderTopLeftRadius
                | Self::BorderTopRightRadius
                | Self::BorderBottomLeftRadius
                | Self::BorderBottomRightRadius
                | Self::BorderStartStartRadius
                | Self::BorderStartEndRadius
                | Self::BorderEndStartRadius
                | Self::BorderEndEndRadius
                | Self::VerticalAlign
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_styles_exclude_box_and_paragraph_layout() {
        for property in [
            StyleProperty::Width,
            StyleProperty::Padding,
            StyleProperty::Display,
            StyleProperty::TextAlign,
            StyleProperty::LineHeight,
        ] {
            assert!(!property.supports_text_scope(TextStyleScope::Run));
            assert!(property.supports_text_scope(TextStyleScope::Paragraph));
            assert!(property.supports_text_scope(TextStyleScope::Attachment));
        }
        for property in [
            StyleProperty::FontSize,
            StyleProperty::VerticalAlign,
            StyleProperty::BackgroundColor,
            StyleProperty::TextDecoration,
        ] {
            assert!(property.supports_text_scope(TextStyleScope::Run));
        }
    }
}
