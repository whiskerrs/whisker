//! Text-related keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/text-align>
//! - <https://lynxjs.org/api/css/properties/text-decoration>
//! - <https://lynxjs.org/api/css/properties/text-overflow>
//! - <https://lynxjs.org/api/css/properties/text-transform>
//! - <https://lynxjs.org/api/css/properties/vertical-align>
//! - <https://lynxjs.org/api/css/properties/direction>
//! - <https://lynxjs.org/api/css/properties/white-space>
//! - <https://lynxjs.org/api/css/properties/word-break>
//! - <https://lynxjs.org/api/css/properties/word-wrap>

use core::fmt;

use crate::to_css::ToCss;

/// The `text-align` keyword. **Lynx does not support `justify`**.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// `left` — align to the left edge.
    Left,
    /// `right` — align to the right edge.
    Right,
    /// `center` — center the text.
    Center,
    /// `start` — align to the logical start (writing-mode aware).
    Start,
    /// `end` — align to the logical end.
    End,
}

impl ToCss for TextAlign {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
            TextAlign::Start => "start",
            TextAlign::End => "end",
        })
    }
}

/// The `text-decoration-line` keyword (one or more values).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextDecorationLine {
    /// `none` — no decoration.
    None,
    /// `underline` — line below the text.
    Underline,
    /// `overline` — line above the text.
    Overline,
    /// `line-through` — line through the middle of the text.
    LineThrough,
}

impl ToCss for TextDecorationLine {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TextDecorationLine::None => "none",
            TextDecorationLine::Underline => "underline",
            TextDecorationLine::Overline => "overline",
            TextDecorationLine::LineThrough => "line-through",
        })
    }
}

/// The `text-decoration-style` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextDecorationStyle {
    /// `solid` — single straight line. Default.
    Solid,
    /// `double` — two parallel lines.
    Double,
    /// `dotted` — line made of dots.
    Dotted,
    /// `dashed` — line made of dashes.
    Dashed,
    /// `wavy` — wavy line.
    Wavy,
}

impl ToCss for TextDecorationStyle {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TextDecorationStyle::Solid => "solid",
            TextDecorationStyle::Double => "double",
            TextDecorationStyle::Dotted => "dotted",
            TextDecorationStyle::Dashed => "dashed",
            TextDecorationStyle::Wavy => "wavy",
        })
    }
}

/// The `text-overflow` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextOverflow {
    /// `clip` — clip overflowing text at the box edge. Default.
    Clip,
    /// `ellipsis` — replace overflowing text with `…`.
    Ellipsis,
}

impl ToCss for TextOverflow {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TextOverflow::Clip => "clip",
            TextOverflow::Ellipsis => "ellipsis",
        })
    }
}

/// The `text-transform` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextTransform {
    /// `none` — no transformation. Default.
    None,
    /// `uppercase` — convert to uppercase.
    Uppercase,
    /// `lowercase` — convert to lowercase.
    Lowercase,
    /// `capitalize` — capitalize the first letter of each word.
    Capitalize,
}

impl ToCss for TextTransform {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            TextTransform::None => "none",
            TextTransform::Uppercase => "uppercase",
            TextTransform::Lowercase => "lowercase",
            TextTransform::Capitalize => "capitalize",
        })
    }
}

/// The `vertical-align` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    /// `baseline` — align to the parent baseline.
    Baseline,
    /// `top` — align to the top of the line box.
    Top,
    /// `middle` — center on the parent's center.
    Middle,
    /// `bottom` — align to the bottom of the line box.
    Bottom,
    /// `super` — raise to superscript.
    Super,
    /// `sub` — lower to subscript.
    Sub,
}

impl ToCss for VerticalAlign {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            VerticalAlign::Baseline => "baseline",
            VerticalAlign::Top => "top",
            VerticalAlign::Middle => "middle",
            VerticalAlign::Bottom => "bottom",
            VerticalAlign::Super => "super",
            VerticalAlign::Sub => "sub",
        })
    }
}

/// The `direction` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// `ltr` — left-to-right. Default.
    Ltr,
    /// `rtl` — right-to-left.
    Rtl,
}

impl ToCss for Direction {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Direction::Ltr => "ltr",
            Direction::Rtl => "rtl",
        })
    }
}

/// The `white-space` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WhiteSpace {
    /// `normal` — collapse whitespace, allow wrapping. Default.
    Normal,
    /// `nowrap` — collapse whitespace, no wrapping.
    Nowrap,
    /// `pre` — preserve whitespace, no wrapping.
    Pre,
    /// `pre-wrap` — preserve whitespace, allow wrapping.
    PreWrap,
    /// `pre-line` — collapse whitespace, preserve line breaks.
    PreLine,
}

impl ToCss for WhiteSpace {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            WhiteSpace::Normal => "normal",
            WhiteSpace::Nowrap => "nowrap",
            WhiteSpace::Pre => "pre",
            WhiteSpace::PreWrap => "pre-wrap",
            WhiteSpace::PreLine => "pre-line",
        })
    }
}

/// The `word-break` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WordBreak {
    /// `normal` — break at standard locations. Default.
    Normal,
    /// `break-all` — break at any character.
    BreakAll,
    /// `keep-all` — never break at CJK punctuation.
    KeepAll,
}

impl ToCss for WordBreak {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            WordBreak::Normal => "normal",
            WordBreak::BreakAll => "break-all",
            WordBreak::KeepAll => "keep-all",
        })
    }
}

/// The `word-wrap` (aka `overflow-wrap`) keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WordWrap {
    /// `normal` — break at allowed break points only. Default.
    Normal,
    /// `break-word` — break long words to prevent overflow.
    BreakWord,
}

impl ToCss for WordWrap {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            WordWrap::Normal => "normal",
            WordWrap::BreakWord => "break-word",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_keyword_set {
        ($cases:expr_2021) => {
            for (k, expected) in $cases {
                assert_eq!(k.to_css_string(), expected);
            }
        };
    }

    #[test]
    fn text_align_all() {
        assert_keyword_set!([
            (TextAlign::Left, "left"),
            (TextAlign::Right, "right"),
            (TextAlign::Center, "center"),
            (TextAlign::Start, "start"),
            (TextAlign::End, "end"),
        ]);
    }

    #[test]
    fn text_decoration_line_all() {
        assert_keyword_set!([
            (TextDecorationLine::None, "none"),
            (TextDecorationLine::Underline, "underline"),
            (TextDecorationLine::Overline, "overline"),
            (TextDecorationLine::LineThrough, "line-through"),
        ]);
    }

    #[test]
    fn text_decoration_style_all() {
        assert_keyword_set!([
            (TextDecorationStyle::Solid, "solid"),
            (TextDecorationStyle::Double, "double"),
            (TextDecorationStyle::Dotted, "dotted"),
            (TextDecorationStyle::Dashed, "dashed"),
            (TextDecorationStyle::Wavy, "wavy"),
        ]);
    }

    #[test]
    fn text_overflow_all() {
        assert_keyword_set!([
            (TextOverflow::Clip, "clip"),
            (TextOverflow::Ellipsis, "ellipsis"),
        ]);
    }

    #[test]
    fn text_transform_all() {
        assert_keyword_set!([
            (TextTransform::None, "none"),
            (TextTransform::Uppercase, "uppercase"),
            (TextTransform::Lowercase, "lowercase"),
            (TextTransform::Capitalize, "capitalize"),
        ]);
    }

    #[test]
    fn vertical_align_all() {
        assert_keyword_set!([
            (VerticalAlign::Baseline, "baseline"),
            (VerticalAlign::Top, "top"),
            (VerticalAlign::Middle, "middle"),
            (VerticalAlign::Bottom, "bottom"),
            (VerticalAlign::Super, "super"),
            (VerticalAlign::Sub, "sub"),
        ]);
    }

    #[test]
    fn direction_all() {
        assert_keyword_set!([(Direction::Ltr, "ltr"), (Direction::Rtl, "rtl")]);
    }

    #[test]
    fn white_space_all() {
        assert_keyword_set!([
            (WhiteSpace::Normal, "normal"),
            (WhiteSpace::Nowrap, "nowrap"),
            (WhiteSpace::Pre, "pre"),
            (WhiteSpace::PreWrap, "pre-wrap"),
            (WhiteSpace::PreLine, "pre-line"),
        ]);
    }

    #[test]
    fn word_break_all() {
        assert_keyword_set!([
            (WordBreak::Normal, "normal"),
            (WordBreak::BreakAll, "break-all"),
            (WordBreak::KeepAll, "keep-all"),
        ]);
    }

    #[test]
    fn word_wrap_all() {
        assert_keyword_set!([
            (WordWrap::Normal, "normal"),
            (WordWrap::BreakWord, "break-word"),
        ]);
    }
}
