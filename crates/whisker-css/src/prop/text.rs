//! Text-content properties: alignment, decoration, transform,
//! overflow, vertical alignment, whitespace handling.

use crate::css::Css;
use crate::data_type::{Color, Length, LengthPercentage};
use crate::keyword::{
    TextAlign, TextDecorationLine, TextDecorationStyle, TextOverflow, TextTransform, VerticalAlign,
    WhiteSpace, WordBreak, WordWrap,
};
use crate::style_value::ToStyleValue;
use crate::to_css::ToCss;

impl Css {
    /// Sets the Lynx-compatible single `text-shadow` layer.
    /// <https://lynxjs.org/api/css/properties/text-shadow>
    pub fn text_shadow(
        self,
        offset_x: Length,
        offset_y: Length,
        blur_radius: Length,
        color: Color,
    ) -> Self {
        use crate::to_css::ToCss;
        let whisker_style::StyleValue::Length(offset_x_value) = offset_x.to_style_value() else {
            unreachable!()
        };
        let whisker_style::StyleValue::Length(offset_y_value) = offset_y.to_style_value() else {
            unreachable!()
        };
        let whisker_style::StyleValue::Length(blur_radius_value) = blur_radius.to_style_value()
        else {
            unreachable!()
        };
        let whisker_style::StyleValue::Color(color_value) = color.to_style_value() else {
            unreachable!()
        };
        let mut css = String::new();
        let _ = offset_x.to_css(&mut css);
        css.push(' ');
        let _ = offset_y.to_css(&mut css);
        css.push(' ');
        let _ = blur_radius.to_css(&mut css);
        css.push(' ');
        let _ = color.to_css(&mut css);
        self.push_semantic(
            crate::StyleProperty::TextShadow,
            whisker_style::StyleValue::TextShadow(whisker_style::TextShadowValue::Shadow {
                offset_x: offset_x_value,
                offset_y: offset_y_value,
                blur_radius: blur_radius_value,
                color: color_value,
            }),
            css,
        )
    }

    /// Disables inherited text shadow paint.
    pub fn text_shadow_none(self) -> Self {
        self.push_semantic(
            crate::StyleProperty::TextShadow,
            whisker_style::StyleValue::TextShadow(whisker_style::TextShadowValue::None),
            "none",
        )
    }

    /// Sets Lynx's inherited, single-line `text-decoration` shorthand.
    ///
    /// Lynx supports `underline` or `line-through`, one stroke style, and one
    /// color. Multiple lines and explicit thickness remain outside the core.
    /// <https://lynxjs.org/api/css/properties/text-decoration>
    pub fn text_decoration(
        self,
        line: TextDecorationLine,
        style: TextDecorationStyle,
        color: Color,
    ) -> Self {
        use crate::to_css::ToCss;
        let line_value = match line {
            TextDecorationLine::None => whisker_style::TextDecorationLineValue::None,
            TextDecorationLine::Underline => whisker_style::TextDecorationLineValue::Underline,
            TextDecorationLine::LineThrough => whisker_style::TextDecorationLineValue::LineThrough,
        };
        let style_value = match style {
            TextDecorationStyle::Solid => whisker_style::TextDecorationStyleValue::Solid,
            TextDecorationStyle::Double => whisker_style::TextDecorationStyleValue::Double,
            TextDecorationStyle::Dotted => whisker_style::TextDecorationStyleValue::Dotted,
            TextDecorationStyle::Dashed => whisker_style::TextDecorationStyleValue::Dashed,
            TextDecorationStyle::Wavy => whisker_style::TextDecorationStyleValue::Wavy,
        };
        let whisker_style::StyleValue::Color(color_value) = color.to_style_value() else {
            unreachable!()
        };
        let mut css = String::new();
        let _ = line.to_css(&mut css);
        css.push(' ');
        let _ = style.to_css(&mut css);
        css.push(' ');
        let _ = color.to_css(&mut css);
        self.push_semantic(
            crate::StyleProperty::TextDecoration,
            whisker_style::StyleValue::TextDecoration(whisker_style::TextDecorationValue {
                line: line_value,
                style: style_value,
                color: Some(color_value),
            }),
            css,
        )
    }

    /// Disables inherited text decoration.
    pub fn text_decoration_none(self) -> Self {
        self.push_semantic(
            crate::StyleProperty::TextDecoration,
            whisker_style::StyleValue::TextDecoration(whisker_style::TextDecorationValue {
                line: whisker_style::TextDecorationLineValue::None,
                style: whisker_style::TextDecorationStyleValue::Solid,
                color: None,
            }),
            "none",
        )
    }

    /// Sets `text-align`. **`justify` is not supported by Lynx**.
    /// <https://lynxjs.org/api/css/properties/text-align>
    pub fn text_align(self, v: TextAlign) -> Self {
        let value = match v {
            TextAlign::Left => whisker_style::TextAlignValue::Left,
            TextAlign::Right => whisker_style::TextAlignValue::Right,
            TextAlign::Center => whisker_style::TextAlignValue::Center,
            TextAlign::Start => whisker_style::TextAlignValue::Start,
            TextAlign::End => whisker_style::TextAlignValue::End,
        };
        self.push_semantic(
            crate::StyleProperty::TextAlign,
            whisker_style::StyleValue::TextAlign(value),
            v.to_css_string(),
        )
    }

    /// Sets `text-decoration-line` (single value).
    /// <https://lynxjs.org/api/css/properties/text-decoration-line>
    pub fn text_decoration_line(self, v: TextDecorationLine) -> Self {
        self.push(crate::StyleProperty::TextDecorationLine, v)
    }

    /// Sets `text-decoration-style`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-style>
    pub fn text_decoration_style(self, v: TextDecorationStyle) -> Self {
        self.push(crate::StyleProperty::TextDecorationStyle, v)
    }

    /// Sets `text-decoration-color`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-color>
    pub fn text_decoration_color(self, v: Color) -> Self {
        self.push(crate::StyleProperty::TextDecorationColor, v)
    }

    /// Sets `text-decoration-thickness`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-thickness>
    pub fn text_decoration_thickness(self, v: Length) -> Self {
        self.push_typed(crate::StyleProperty::TextDecorationThickness, v)
    }

    /// Sets `text-overflow`.
    /// <https://lynxjs.org/api/css/properties/text-overflow>
    pub fn text_overflow(self, v: TextOverflow) -> Self {
        let value = match v {
            TextOverflow::Clip => whisker_style::TextOverflowValue::Clip,
            TextOverflow::Ellipsis => whisker_style::TextOverflowValue::Ellipsis,
        };
        self.push_semantic(
            crate::StyleProperty::TextOverflow,
            whisker_style::StyleValue::TextOverflow(value),
            v.to_css_string(),
        )
    }

    /// Sets `text-transform`.
    /// <https://lynxjs.org/api/css/properties/text-transform>
    pub fn text_transform(self, v: TextTransform) -> Self {
        self.push(crate::StyleProperty::TextTransform, v)
    }

    /// Sets `text-indent` — first-line indentation.
    /// <https://lynxjs.org/api/css/properties/text-indent>
    pub fn text_indent(self, v: impl Into<LengthPercentage>) -> Self {
        self.push_typed(crate::StyleProperty::TextIndent, v.into())
    }

    /// Sets `vertical-align`.
    /// <https://lynxjs.org/api/css/properties/vertical-align>
    pub fn vertical_align(self, v: VerticalAlign) -> Self {
        self.push(crate::StyleProperty::VerticalAlign, v)
    }

    /// Sets `white-space`.
    /// <https://lynxjs.org/api/css/properties/white-space>
    pub fn white_space(self, v: WhiteSpace) -> Self {
        let value = match v {
            WhiteSpace::Normal => whisker_style::WhiteSpaceValue::Normal,
            WhiteSpace::Nowrap => whisker_style::WhiteSpaceValue::NoWrap,
            WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine => {
                return self.push(crate::StyleProperty::WhiteSpace, v);
            }
        };
        self.push_semantic(
            crate::StyleProperty::WhiteSpace,
            whisker_style::StyleValue::WhiteSpace(value),
            v.to_css_string(),
        )
    }

    /// Sets `word-break`.
    /// <https://lynxjs.org/api/css/properties/word-break>
    pub fn word_break(self, v: WordBreak) -> Self {
        let value = match v {
            WordBreak::Normal => whisker_style::WordBreakValue::Normal,
            WordBreak::BreakAll => whisker_style::WordBreakValue::BreakAll,
            WordBreak::KeepAll => whisker_style::WordBreakValue::KeepAll,
        };
        self.push_semantic(
            crate::StyleProperty::WordBreak,
            whisker_style::StyleValue::WordBreak(value),
            v.to_css_string(),
        )
    }

    /// Sets `overflow-wrap`.
    /// <https://lynxjs.org/api/css/properties/overflow-wrap>
    pub fn overflow_wrap(self, v: WordWrap) -> Self {
        self.push(crate::StyleProperty::OverflowWrap, v)
    }
}

#[cfg(test)]
mod tests {
    use crate::Css;
    use crate::data_type::{Color, NamedColor};
    use crate::ext::*;
    use crate::keyword::*;

    #[test]
    fn text_align_keywords() {
        let s = Css::new().text_align(TextAlign::Center);
        assert_eq!(s.to_string(), "text-align: center;");
        assert_eq!(
            s.to_specified_style().unwrap().resolved()[0].value(),
            &whisker_style::StyleValue::TextAlign(whisker_style::TextAlignValue::Center)
        );
    }

    #[test]
    fn single_text_shadow_is_typed_and_uses_lynx_order() {
        let style = Css::new().text_shadow(1.px(), 2.px(), 3.px(), Color::rgba(255, 0, 0, 0.5));
        assert_eq!(
            style.to_string(),
            "text-shadow: 1px 2px 3px rgba(255, 0, 0, 0.5);"
        );
        let specified = style.to_specified_style().unwrap();
        assert!(matches!(
            specified.resolved()[0].value(),
            whisker_style::StyleValue::TextShadow(whisker_style::TextShadowValue::Shadow { .. })
        ));
        assert_eq!(
            Css::new().text_shadow_none().to_string(),
            "text-shadow: none;"
        );
    }

    #[test]
    fn lynx_text_decoration_shorthand_is_typed() {
        let style = Css::new().text_decoration(
            TextDecorationLine::LineThrough,
            TextDecorationStyle::Dashed,
            Color::Named(NamedColor::Red),
        );
        assert_eq!(
            style.to_string(),
            "text-decoration: line-through dashed red;"
        );
        assert!(matches!(
            style.to_specified_style().unwrap().resolved()[0].value(),
            whisker_style::StyleValue::TextDecoration(whisker_style::TextDecorationValue {
                line: whisker_style::TextDecorationLineValue::LineThrough,
                style: whisker_style::TextDecorationStyleValue::Dashed,
                ..
            })
        ));
        assert_eq!(
            Css::new().text_decoration_none().to_string(),
            "text-decoration: none;"
        );
    }

    #[test]
    fn text_decoration_set() {
        let s = Css::new()
            .text_decoration_line(TextDecorationLine::Underline)
            .text_decoration_style(TextDecorationStyle::Wavy)
            .text_decoration_color(Color::Named(NamedColor::Red))
            .text_decoration_thickness(2.px());
        assert_eq!(
            s.to_string(),
            "text-decoration-line: underline; text-decoration-style: wavy; text-decoration-color: red; text-decoration-thickness: 2px;"
        );
    }

    #[test]
    fn text_overflow_and_transform() {
        let s = Css::new()
            .text_overflow(TextOverflow::Ellipsis)
            .text_transform(TextTransform::Uppercase);
        assert_eq!(
            s.to_string(),
            "text-overflow: ellipsis; text-transform: uppercase;"
        );
        let semantic = Css::new().text_overflow(TextOverflow::Ellipsis);
        assert!(matches!(
            semantic.to_specified_style().unwrap().resolved()[0].value(),
            whisker_style::StyleValue::TextOverflow(whisker_style::TextOverflowValue::Ellipsis)
        ));
    }

    #[test]
    fn text_indent_value() {
        let s = Css::new().text_indent(px(20));
        assert_eq!(s.to_string(), "text-indent: 20px;");
        assert!(matches!(
            s.to_specified_style().unwrap().resolved()[0].value(),
            whisker_style::StyleValue::LengthPercentage(_)
        ));
    }

    #[test]
    fn vertical_align_keywords() {
        let s = Css::new().vertical_align(VerticalAlign::Middle);
        assert_eq!(s.to_string(), "vertical-align: middle;");
    }

    #[test]
    fn whitespace_word_handling() {
        let s = Css::new()
            .white_space(WhiteSpace::Nowrap)
            .word_break(WordBreak::BreakAll)
            .overflow_wrap(WordWrap::BreakWord);
        assert_eq!(
            s.to_string(),
            "white-space: nowrap; word-break: break-all; overflow-wrap: break-word;"
        );
        let specified = Css::new()
            .white_space(WhiteSpace::Nowrap)
            .word_break(WordBreak::BreakAll)
            .to_specified_style()
            .unwrap();
        assert!(matches!(
            specified.resolved()[0].value(),
            whisker_style::StyleValue::WhiteSpace(whisker_style::WhiteSpaceValue::NoWrap)
        ));
        assert!(matches!(
            specified.resolved()[1].value(),
            whisker_style::StyleValue::WordBreak(whisker_style::WordBreakValue::BreakAll)
        ));

        for unsupported in [WhiteSpace::Pre, WhiteSpace::PreWrap, WhiteSpace::PreLine] {
            assert!(
                Css::new()
                    .white_space(unsupported)
                    .to_specified_style()
                    .is_err()
            );
        }
    }
}
