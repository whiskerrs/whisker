//! Keyword enums for Lynx's `linear-*` layout extension.
//!
//! Lynx ships its own linear-layout container as an alternative to
//! CSS flexbox. The keywords are Lynx-specific and have no MDN
//! equivalent. References:
//! - <https://lynxjs.org/api/css/properties/linear-direction>
//! - <https://lynxjs.org/api/css/properties/linear-orientation>
//! - <https://lynxjs.org/api/css/properties/linear-gravity>
//! - <https://lynxjs.org/api/css/properties/linear-cross-gravity>
//! - <https://lynxjs.org/api/css/properties/linear-layout-gravity>

use core::fmt;

use crate::to_css::ToCss;

/// The `linear-orientation` keyword. Lynx-specific replacement for
/// `flex-direction` when using `display: linear`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinearOrientation {
    /// `horizontal` — main axis runs horizontally.
    Horizontal,
    /// `vertical` — main axis runs vertically.
    Vertical,
    /// `horizontal-reverse` — horizontal, reversed.
    HorizontalReverse,
    /// `vertical-reverse` — vertical, reversed.
    VerticalReverse,
}

impl ToCss for LinearOrientation {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            LinearOrientation::Horizontal => "horizontal",
            LinearOrientation::Vertical => "vertical",
            LinearOrientation::HorizontalReverse => "horizontal-reverse",
            LinearOrientation::VerticalReverse => "vertical-reverse",
        })
    }
}

/// The `linear-gravity` keyword. Lynx-specific alignment along the
/// main axis. **`linear-gravity` is deprecated**; prefer
/// `justify-content` on a `display: flex` container instead.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinearGravity {
    /// `none` — no gravity. Default.
    None,
    /// `top` — gravitate to the top edge.
    Top,
    /// `bottom` — gravitate to the bottom edge.
    Bottom,
    /// `left` — gravitate to the left edge.
    Left,
    /// `right` — gravitate to the right edge.
    Right,
    /// `center-vertical` — center along the vertical axis.
    CenterVertical,
    /// `center-horizontal` — center along the horizontal axis.
    CenterHorizontal,
    /// `space-between` — equal space between items.
    SpaceBetween,
    /// `start` — pack to the start edge.
    Start,
    /// `end` — pack to the end edge.
    End,
    /// `center` — pack to the center.
    Center,
}

impl ToCss for LinearGravity {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            LinearGravity::None => "none",
            LinearGravity::Top => "top",
            LinearGravity::Bottom => "bottom",
            LinearGravity::Left => "left",
            LinearGravity::Right => "right",
            LinearGravity::CenterVertical => "center-vertical",
            LinearGravity::CenterHorizontal => "center-horizontal",
            LinearGravity::SpaceBetween => "space-between",
            LinearGravity::Start => "start",
            LinearGravity::End => "end",
            LinearGravity::Center => "center",
        })
    }
}

/// The `linear-cross-gravity` keyword. Lynx-specific alignment along
/// the cross axis. Comparable to `align-items` in flexbox.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinearCrossGravity {
    /// `none` — no cross-axis alignment. Default.
    None,
    /// `start` — align to the cross-axis start.
    Start,
    /// `end` — align to the cross-axis end.
    End,
    /// `center` — center along the cross axis.
    Center,
    /// `stretch` — stretch across the cross axis.
    Stretch,
}

impl ToCss for LinearCrossGravity {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            LinearCrossGravity::None => "none",
            LinearCrossGravity::Start => "start",
            LinearCrossGravity::End => "end",
            LinearCrossGravity::Center => "center",
            LinearCrossGravity::Stretch => "stretch",
        })
    }
}

/// The `linear-layout-gravity` keyword. Lynx-specific per-child
/// override for cross-axis alignment.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinearLayoutGravity {
    /// `none` — defer to parent's `linear-cross-gravity`. Default.
    None,
    /// `start` — align to the cross-axis start.
    Start,
    /// `end` — align to the cross-axis end.
    End,
    /// `center` — center along the cross axis.
    Center,
    /// `stretch` — stretch across the cross axis.
    Stretch,
}

impl ToCss for LinearLayoutGravity {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            LinearLayoutGravity::None => "none",
            LinearLayoutGravity::Start => "start",
            LinearLayoutGravity::End => "end",
            LinearLayoutGravity::Center => "center",
            LinearLayoutGravity::Stretch => "stretch",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_orientation_all() {
        let cases = [
            (LinearOrientation::Horizontal, "horizontal"),
            (LinearOrientation::Vertical, "vertical"),
            (LinearOrientation::HorizontalReverse, "horizontal-reverse"),
            (LinearOrientation::VerticalReverse, "vertical-reverse"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn linear_gravity_all() {
        let cases = [
            (LinearGravity::None, "none"),
            (LinearGravity::Top, "top"),
            (LinearGravity::Bottom, "bottom"),
            (LinearGravity::Left, "left"),
            (LinearGravity::Right, "right"),
            (LinearGravity::CenterVertical, "center-vertical"),
            (LinearGravity::CenterHorizontal, "center-horizontal"),
            (LinearGravity::SpaceBetween, "space-between"),
            (LinearGravity::Start, "start"),
            (LinearGravity::End, "end"),
            (LinearGravity::Center, "center"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn linear_cross_gravity_all() {
        let cases = [
            (LinearCrossGravity::None, "none"),
            (LinearCrossGravity::Start, "start"),
            (LinearCrossGravity::End, "end"),
            (LinearCrossGravity::Center, "center"),
            (LinearCrossGravity::Stretch, "stretch"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn linear_layout_gravity_all() {
        let cases = [
            (LinearLayoutGravity::None, "none"),
            (LinearLayoutGravity::Start, "start"),
            (LinearLayoutGravity::End, "end"),
            (LinearLayoutGravity::Center, "center"),
            (LinearLayoutGravity::Stretch, "stretch"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }
}
