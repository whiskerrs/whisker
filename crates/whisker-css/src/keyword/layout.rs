//! Layout-related keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/display>
//! - <https://lynxjs.org/api/css/properties/position>
//! - <https://lynxjs.org/api/css/properties/overflow>
//! - <https://lynxjs.org/api/css/properties/visibility>
//! - <https://lynxjs.org/api/css/properties/box-sizing>
//! - <https://lynxjs.org/api/css/properties/pointer-events>

use core::fmt;

use crate::to_css::ToCss;

/// The `display` keyword. Lynx's default for `<view>` is
/// [`Display::Linear`] (Lynx's vertical/horizontal stacking layout);
/// `flex` is required to opt into CSS flexbox semantics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Display {
    /// `none` — element is removed from the layout tree.
    None,
    /// `flex` — CSS flexbox layout.
    Flex,
    /// `grid` — CSS grid layout.
    Grid,
    /// `linear` — Lynx's linear layout (default for `<view>`).
    Linear,
    /// `relative` — Lynx's relative-positioning container.
    Relative,
}

impl ToCss for Display {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Display::None => "none",
            Display::Flex => "flex",
            Display::Grid => "grid",
            Display::Linear => "linear",
            Display::Relative => "relative",
        })
    }
}

/// The `position` keyword. **Lynx does not support `static`** — the
/// default in Lynx is `relative`, so a `static` value is meaningless
/// and is omitted from this enum.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PositionKind {
    /// `relative` — positioned with normal flow as origin (default).
    Relative,
    /// `absolute` — positioned with the containing block as origin.
    Absolute,
    /// `fixed` — positioned with the viewport as origin.
    Fixed,
    /// `sticky` — switches between `relative` and `fixed` based on
    /// scroll position.
    Sticky,
}

impl ToCss for PositionKind {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            PositionKind::Relative => "relative",
            PositionKind::Absolute => "absolute",
            PositionKind::Fixed => "fixed",
            PositionKind::Sticky => "sticky",
        })
    }
}

/// The `overflow` keyword. **Lynx supports only two values** —
/// `visible` (default) and `hidden`. CSS's `scroll` and `auto` are
/// **not** supported; use a `<scroll-view>` element for scrolling.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Overflow {
    /// `visible` — content overflows the box. Default.
    Visible,
    /// `hidden` — content is clipped to the box.
    Hidden,
}

impl ToCss for Overflow {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Overflow::Visible => "visible",
            Overflow::Hidden => "hidden",
        })
    }
}

/// The `visibility` keyword. **Lynx does not support `collapse`**.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// `visible` — element is rendered. Default.
    Visible,
    /// `hidden` — element is invisible but still occupies space.
    Hidden,
}

impl ToCss for Visibility {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Visibility::Visible => "visible",
            Visibility::Hidden => "hidden",
        })
    }
}

/// The `box-sizing` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    /// `content-box` — `width`/`height` apply to the content box only.
    ContentBox,
    /// `border-box` — `width`/`height` include padding and border.
    BorderBox,
}

impl ToCss for BoxSizing {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BoxSizing::ContentBox => "content-box",
            BoxSizing::BorderBox => "border-box",
        })
    }
}

/// The `pointer-events` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointerEvents {
    /// `auto` — element receives pointer events.
    Auto,
    /// `none` — element is invisible to pointer events; events pass
    /// through.
    None,
}

impl ToCss for PointerEvents {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            PointerEvents::Auto => "auto",
            PointerEvents::None => "none",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_keywords() {
        let cases = [
            (Display::None, "none"),
            (Display::Flex, "flex"),
            (Display::Grid, "grid"),
            (Display::Linear, "linear"),
            (Display::Relative, "relative"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn position_keywords() {
        let cases = [
            (PositionKind::Relative, "relative"),
            (PositionKind::Absolute, "absolute"),
            (PositionKind::Fixed, "fixed"),
            (PositionKind::Sticky, "sticky"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }

    #[test]
    fn overflow_keywords() {
        assert_eq!(Overflow::Visible.to_css_string(), "visible");
        assert_eq!(Overflow::Hidden.to_css_string(), "hidden");
    }

    #[test]
    fn visibility_keywords() {
        assert_eq!(Visibility::Visible.to_css_string(), "visible");
        assert_eq!(Visibility::Hidden.to_css_string(), "hidden");
    }

    #[test]
    fn box_sizing_keywords() {
        assert_eq!(BoxSizing::ContentBox.to_css_string(), "content-box");
        assert_eq!(BoxSizing::BorderBox.to_css_string(), "border-box");
    }

    #[test]
    fn pointer_events_keywords() {
        assert_eq!(PointerEvents::Auto.to_css_string(), "auto");
        assert_eq!(PointerEvents::None.to_css_string(), "none");
    }
}
