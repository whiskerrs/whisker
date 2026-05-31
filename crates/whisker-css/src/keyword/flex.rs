//! Flexbox keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/flex-direction>
//! - <https://lynxjs.org/api/css/properties/flex-wrap>
//! - <https://lynxjs.org/api/css/properties/justify-content>
//! - <https://lynxjs.org/api/css/properties/align-items>
//! - <https://lynxjs.org/api/css/properties/align-self>
//! - <https://lynxjs.org/api/css/properties/align-content>

use core::fmt;

use crate::to_css::ToCss;

/// The `flex-direction` keyword. Lynx accepts the four standard
/// values; the deprecated `vertical` and `horizontal` aliases are
/// **not** included.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    /// `row` — main axis runs left-to-right. Default.
    Row,
    /// `row-reverse` — main axis runs right-to-left.
    RowReverse,
    /// `column` — main axis runs top-to-bottom.
    Column,
    /// `column-reverse` — main axis runs bottom-to-top.
    ColumnReverse,
}

impl ToCss for FlexDirection {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            FlexDirection::Row => "row",
            FlexDirection::RowReverse => "row-reverse",
            FlexDirection::Column => "column",
            FlexDirection::ColumnReverse => "column-reverse",
        })
    }
}

/// The `flex-wrap` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    /// `nowrap` — single line, no wrapping. Default.
    Nowrap,
    /// `wrap` — multi-line, wrap forward.
    Wrap,
    /// `wrap-reverse` — multi-line, wrap reverse.
    WrapReverse,
}

impl ToCss for FlexWrap {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            FlexWrap::Nowrap => "nowrap",
            FlexWrap::Wrap => "wrap",
            FlexWrap::WrapReverse => "wrap-reverse",
        })
    }
}

/// The `justify-content` keyword. **Lynx does not support `normal`,
/// `left`, `right`, or the `baseline*` family.**
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum JustifyContent {
    /// `stretch` — distribute remaining space to flexible items.
    Stretch,
    /// `flex-start` — pack at the start of the main axis. Default.
    FlexStart,
    /// `flex-end` — pack at the end of the main axis.
    FlexEnd,
    /// `center` — pack around the center of the main axis.
    Center,
    /// `space-between` — equal space between items.
    SpaceBetween,
    /// `space-around` — equal space around items.
    SpaceAround,
    /// `space-evenly` — equal space between and around items.
    SpaceEvenly,
    /// `start` — pack at the logical start (writing-mode aware).
    Start,
    /// `end` — pack at the logical end.
    End,
}

impl ToCss for JustifyContent {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            JustifyContent::Stretch => "stretch",
            JustifyContent::FlexStart => "flex-start",
            JustifyContent::FlexEnd => "flex-end",
            JustifyContent::Center => "center",
            JustifyContent::SpaceBetween => "space-between",
            JustifyContent::SpaceAround => "space-around",
            JustifyContent::SpaceEvenly => "space-evenly",
            JustifyContent::Start => "start",
            JustifyContent::End => "end",
        })
    }
}

/// The `align-items` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AlignItems {
    /// `stretch` — stretch items to fill the cross axis. Default.
    Stretch,
    /// `flex-start` — pack at the start of the cross axis.
    FlexStart,
    /// `flex-end` — pack at the end of the cross axis.
    FlexEnd,
    /// `center` — pack around the center of the cross axis.
    Center,
    /// `baseline` — align baselines of items.
    Baseline,
    /// `start` — pack at the logical start.
    Start,
    /// `end` — pack at the logical end.
    End,
}

impl ToCss for AlignItems {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AlignItems::Stretch => "stretch",
            AlignItems::FlexStart => "flex-start",
            AlignItems::FlexEnd => "flex-end",
            AlignItems::Center => "center",
            AlignItems::Baseline => "baseline",
            AlignItems::Start => "start",
            AlignItems::End => "end",
        })
    }
}

/// The `align-self` keyword. Adds `auto` (defer to the container's
/// `align-items`) to the [`AlignItems`] set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AlignSelf {
    /// `auto` — inherit `align-items` from the parent. Default.
    Auto,
    /// `stretch` — stretch this item across the cross axis.
    Stretch,
    /// `flex-start` — pack at the start of the cross axis.
    FlexStart,
    /// `flex-end` — pack at the end of the cross axis.
    FlexEnd,
    /// `center` — pack around the center of the cross axis.
    Center,
    /// `baseline` — align baseline with sibling items.
    Baseline,
    /// `start` — pack at the logical start.
    Start,
    /// `end` — pack at the logical end.
    End,
}

impl ToCss for AlignSelf {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AlignSelf::Auto => "auto",
            AlignSelf::Stretch => "stretch",
            AlignSelf::FlexStart => "flex-start",
            AlignSelf::FlexEnd => "flex-end",
            AlignSelf::Center => "center",
            AlignSelf::Baseline => "baseline",
            AlignSelf::Start => "start",
            AlignSelf::End => "end",
        })
    }
}

/// The `align-content` keyword. Used to distribute extra cross-axis
/// space when items wrap onto multiple lines.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum AlignContent {
    /// `stretch` — stretch lines to fill remaining space. Default.
    Stretch,
    /// `flex-start` — pack lines at the start.
    FlexStart,
    /// `flex-end` — pack lines at the end.
    FlexEnd,
    /// `center` — center the lines.
    Center,
    /// `space-between` — equal space between lines.
    SpaceBetween,
    /// `space-around` — equal space around each line.
    SpaceAround,
    /// `space-evenly` — equal space between and around lines.
    SpaceEvenly,
}

impl ToCss for AlignContent {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            AlignContent::Stretch => "stretch",
            AlignContent::FlexStart => "flex-start",
            AlignContent::FlexEnd => "flex-end",
            AlignContent::Center => "center",
            AlignContent::SpaceBetween => "space-between",
            AlignContent::SpaceAround => "space-around",
            AlignContent::SpaceEvenly => "space-evenly",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_direction_all() {
        let cases = [
            (FlexDirection::Row, "row"),
            (FlexDirection::RowReverse, "row-reverse"),
            (FlexDirection::Column, "column"),
            (FlexDirection::ColumnReverse, "column-reverse"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn flex_wrap_all() {
        let cases = [
            (FlexWrap::Nowrap, "nowrap"),
            (FlexWrap::Wrap, "wrap"),
            (FlexWrap::WrapReverse, "wrap-reverse"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn justify_content_all() {
        let cases = [
            (JustifyContent::Stretch, "stretch"),
            (JustifyContent::FlexStart, "flex-start"),
            (JustifyContent::FlexEnd, "flex-end"),
            (JustifyContent::Center, "center"),
            (JustifyContent::SpaceBetween, "space-between"),
            (JustifyContent::SpaceAround, "space-around"),
            (JustifyContent::SpaceEvenly, "space-evenly"),
            (JustifyContent::Start, "start"),
            (JustifyContent::End, "end"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn align_items_all() {
        let cases = [
            (AlignItems::Stretch, "stretch"),
            (AlignItems::FlexStart, "flex-start"),
            (AlignItems::FlexEnd, "flex-end"),
            (AlignItems::Center, "center"),
            (AlignItems::Baseline, "baseline"),
            (AlignItems::Start, "start"),
            (AlignItems::End, "end"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn align_self_all() {
        let cases = [
            (AlignSelf::Auto, "auto"),
            (AlignSelf::Stretch, "stretch"),
            (AlignSelf::FlexStart, "flex-start"),
            (AlignSelf::FlexEnd, "flex-end"),
            (AlignSelf::Center, "center"),
            (AlignSelf::Baseline, "baseline"),
            (AlignSelf::Start, "start"),
            (AlignSelf::End, "end"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn align_content_all() {
        let cases = [
            (AlignContent::Stretch, "stretch"),
            (AlignContent::FlexStart, "flex-start"),
            (AlignContent::FlexEnd, "flex-end"),
            (AlignContent::Center, "center"),
            (AlignContent::SpaceBetween, "space-between"),
            (AlignContent::SpaceAround, "space-around"),
            (AlignContent::SpaceEvenly, "space-evenly"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }
}
