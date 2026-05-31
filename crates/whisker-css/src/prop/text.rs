//! Text-content properties: alignment, decoration, transform,
//! overflow, vertical alignment, whitespace handling.

use crate::css::Css;
use crate::data_type::{Color, Length, LengthPercentage};
use crate::keyword::{
    TextAlign, TextDecorationLine, TextDecorationStyle, TextOverflow, TextTransform, VerticalAlign,
    WhiteSpace, WordBreak, WordWrap,
};

impl Css {
    /// Sets `text-align`. **`justify` is not supported by Lynx**.
    /// <https://lynxjs.org/api/css/properties/text-align>
    pub fn text_align(self, v: TextAlign) -> Self {
        self.push("text-align", v)
    }

    /// Sets `text-decoration-line` (single value).
    /// <https://lynxjs.org/api/css/properties/text-decoration-line>
    pub fn text_decoration_line(self, v: TextDecorationLine) -> Self {
        self.push("text-decoration-line", v)
    }

    /// Sets `text-decoration-style`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-style>
    pub fn text_decoration_style(self, v: TextDecorationStyle) -> Self {
        self.push("text-decoration-style", v)
    }

    /// Sets `text-decoration-color`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-color>
    pub fn text_decoration_color(self, v: Color) -> Self {
        self.push("text-decoration-color", v)
    }

    /// Sets `text-decoration-thickness`.
    /// <https://lynxjs.org/api/css/properties/text-decoration-thickness>
    pub fn text_decoration_thickness(self, v: Length) -> Self {
        self.push("text-decoration-thickness", v)
    }

    /// Sets `text-overflow`.
    /// <https://lynxjs.org/api/css/properties/text-overflow>
    pub fn text_overflow(self, v: TextOverflow) -> Self {
        self.push("text-overflow", v)
    }

    /// Sets `text-transform`.
    /// <https://lynxjs.org/api/css/properties/text-transform>
    pub fn text_transform(self, v: TextTransform) -> Self {
        self.push("text-transform", v)
    }

    /// Sets `text-indent` — first-line indentation.
    /// <https://lynxjs.org/api/css/properties/text-indent>
    pub fn text_indent(self, v: impl Into<LengthPercentage>) -> Self {
        self.push("text-indent", v.into())
    }

    /// Sets `vertical-align`.
    /// <https://lynxjs.org/api/css/properties/vertical-align>
    pub fn vertical_align(self, v: VerticalAlign) -> Self {
        self.push("vertical-align", v)
    }

    /// Sets `white-space`.
    /// <https://lynxjs.org/api/css/properties/white-space>
    pub fn white_space(self, v: WhiteSpace) -> Self {
        self.push("white-space", v)
    }

    /// Sets `word-break`.
    /// <https://lynxjs.org/api/css/properties/word-break>
    pub fn word_break(self, v: WordBreak) -> Self {
        self.push("word-break", v)
    }

    /// Sets `word-wrap` (also known as `overflow-wrap`).
    /// <https://lynxjs.org/api/css/properties/word-wrap>
    pub fn word_wrap(self, v: WordWrap) -> Self {
        self.push("word-wrap", v)
    }

    /// Sets `overflow-wrap` — synonym of `word-wrap`.
    /// <https://lynxjs.org/api/css/properties/overflow-wrap>
    pub fn overflow_wrap(self, v: WordWrap) -> Self {
        self.push("overflow-wrap", v)
    }

    /// Sets `-webkit-line-clamp` — limit the visible line count.
    /// <https://lynxjs.org/api/css/properties/-webkit-line-clamp>
    pub fn webkit_line_clamp(self, v: u32) -> Self {
        self.push_raw("-webkit-line-clamp", v.to_string())
    }

    /// Sets `text-stroke-width`.
    /// <https://lynxjs.org/api/css/properties/text-stroke-width>
    pub fn text_stroke_width(self, v: Length) -> Self {
        self.push("text-stroke-width", v)
    }

    /// Sets `text-stroke-color`.
    /// <https://lynxjs.org/api/css/properties/text-stroke-color>
    pub fn text_stroke_color(self, v: Color) -> Self {
        self.push("text-stroke-color", v)
    }
}

#[cfg(test)]
mod tests {
    use crate::data_type::{Color, NamedColor};
    use crate::ext::*;
    use crate::keyword::*;
    use crate::Css;

    #[test]
    fn text_align_keywords() {
        let s = Css::new().text_align(TextAlign::Center);
        assert_eq!(s.to_string(), "text-align: center;");
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
    }

    #[test]
    fn text_indent_value() {
        let s = Css::new().text_indent(px(20));
        assert_eq!(s.to_string(), "text-indent: 20px;");
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
            .word_wrap(WordWrap::BreakWord)
            .overflow_wrap(WordWrap::Normal);
        assert_eq!(
            s.to_string(),
            "white-space: nowrap; word-break: break-all; word-wrap: break-word; overflow-wrap: normal;"
        );
    }

    #[test]
    fn webkit_line_clamp_count() {
        let s = Css::new().webkit_line_clamp(2);
        assert_eq!(s.to_string(), "-webkit-line-clamp: 2;");
    }

    #[test]
    fn text_stroke_set() {
        let s = Css::new()
            .text_stroke_width(1.px())
            .text_stroke_color(Color::hex(0x000000));
        assert_eq!(
            s.to_string(),
            "text-stroke-width: 1px; text-stroke-color: rgb(0, 0, 0);"
        );
    }
}
