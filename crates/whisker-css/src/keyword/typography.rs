//! Typography-related keyword enums (font + cursor).
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/font-style>
//! - <https://lynxjs.org/api/css/properties/font-weight>
//! - <https://lynxjs.org/api/css/properties/font-variant>
//! - <https://lynxjs.org/api/css/properties/cursor>

use core::fmt;

use crate::to_css::ToCss;

/// The `font-style` keyword.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontStyle {
    /// `normal` — upright. Default.
    Normal,
    /// `italic` — italicized.
    Italic,
    /// `oblique` — slanted (typically synthesized from the upright
    /// face when an explicit oblique face is unavailable).
    Oblique,
}

impl ToCss for FontStyle {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            FontStyle::Normal => "normal",
            FontStyle::Italic => "italic",
            FontStyle::Oblique => "oblique",
        })
    }
}

/// The `font-weight` keyword or numeric value. **Lynx does not
/// support `bolder` or `lighter`**.
///
/// Numeric weights are accepted in the standard CSS range `1..=1000`
/// (commonly `100`, `200`, …, `900`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// `normal` — equivalent to `400`. Default.
    Normal,
    /// `bold` — equivalent to `700`.
    Bold,
    /// Numeric weight, typically a multiple of 100.
    Numeric(u16),
}

impl ToCss for FontWeight {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            FontWeight::Normal => dest.write_str("normal"),
            FontWeight::Bold => dest.write_str("bold"),
            FontWeight::Numeric(n) => write!(dest, "{n}"),
        }
    }
}

/// The `font-variant` keyword (Lynx accepts a small subset of the CSS
/// font-variant shorthand).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontVariant {
    /// `normal` — no variant features. Default.
    Normal,
    /// `small-caps` — render lowercase as small capitals.
    SmallCaps,
}

impl ToCss for FontVariant {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            FontVariant::Normal => "normal",
            FontVariant::SmallCaps => "small-caps",
        })
    }
}

/// The `cursor` keyword. The Lynx set tracks the common subset of
/// CSS cursors that map to mobile pointer affordances.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// `auto` — let the platform decide. Default.
    Auto,
    /// `default` — the platform's default pointer.
    Default,
    /// `pointer` — typically a hand, for interactive elements.
    Pointer,
    /// `text` — typically an I-beam, for selectable text.
    Text,
    /// `none` — hide the cursor.
    None,
    /// `context-menu`.
    ContextMenu,
    /// `help`.
    Help,
    /// `progress`.
    Progress,
    /// `wait`.
    Wait,
    /// `cell`.
    Cell,
    /// `crosshair`.
    Crosshair,
    /// `vertical-text`.
    VerticalText,
    /// `alias`.
    Alias,
    /// `copy`.
    Copy,
    /// `move`.
    Move,
    /// `no-drop`.
    NoDrop,
    /// `not-allowed`.
    NotAllowed,
    /// `grab`.
    Grab,
    /// `grabbing`.
    Grabbing,
    /// `col-resize`.
    ColResize,
    /// `row-resize`.
    RowResize,
    /// `n-resize`.
    NResize,
    /// `e-resize`.
    EResize,
    /// `s-resize`.
    SResize,
    /// `w-resize`.
    WResize,
    /// `ne-resize`.
    NeResize,
    /// `nw-resize`.
    NwResize,
    /// `se-resize`.
    SeResize,
    /// `sw-resize`.
    SwResize,
    /// `ew-resize`.
    EwResize,
    /// `ns-resize`.
    NsResize,
    /// `nesw-resize`.
    NeswResize,
    /// `nwse-resize`.
    NwseResize,
    /// `zoom-in`.
    ZoomIn,
    /// `zoom-out`.
    ZoomOut,
}

impl ToCss for Cursor {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            Cursor::Auto => "auto",
            Cursor::Default => "default",
            Cursor::Pointer => "pointer",
            Cursor::Text => "text",
            Cursor::None => "none",
            Cursor::ContextMenu => "context-menu",
            Cursor::Help => "help",
            Cursor::Progress => "progress",
            Cursor::Wait => "wait",
            Cursor::Cell => "cell",
            Cursor::Crosshair => "crosshair",
            Cursor::VerticalText => "vertical-text",
            Cursor::Alias => "alias",
            Cursor::Copy => "copy",
            Cursor::Move => "move",
            Cursor::NoDrop => "no-drop",
            Cursor::NotAllowed => "not-allowed",
            Cursor::Grab => "grab",
            Cursor::Grabbing => "grabbing",
            Cursor::ColResize => "col-resize",
            Cursor::RowResize => "row-resize",
            Cursor::NResize => "n-resize",
            Cursor::EResize => "e-resize",
            Cursor::SResize => "s-resize",
            Cursor::WResize => "w-resize",
            Cursor::NeResize => "ne-resize",
            Cursor::NwResize => "nw-resize",
            Cursor::SeResize => "se-resize",
            Cursor::SwResize => "sw-resize",
            Cursor::EwResize => "ew-resize",
            Cursor::NsResize => "ns-resize",
            Cursor::NeswResize => "nesw-resize",
            Cursor::NwseResize => "nwse-resize",
            Cursor::ZoomIn => "zoom-in",
            Cursor::ZoomOut => "zoom-out",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_style_all() {
        assert_eq!(FontStyle::Normal.to_css_string(), "normal");
        assert_eq!(FontStyle::Italic.to_css_string(), "italic");
        assert_eq!(FontStyle::Oblique.to_css_string(), "oblique");
    }

    #[test]
    fn font_weight_keywords() {
        assert_eq!(FontWeight::Normal.to_css_string(), "normal");
        assert_eq!(FontWeight::Bold.to_css_string(), "bold");
    }

    #[test]
    fn font_weight_numeric() {
        assert_eq!(FontWeight::Numeric(100).to_css_string(), "100");
        assert_eq!(FontWeight::Numeric(700).to_css_string(), "700");
        assert_eq!(FontWeight::Numeric(950).to_css_string(), "950");
    }

    #[test]
    fn font_variant_all() {
        assert_eq!(FontVariant::Normal.to_css_string(), "normal");
        assert_eq!(FontVariant::SmallCaps.to_css_string(), "small-caps");
    }

    #[test]
    fn cursor_all() {
        let cases = [
            (Cursor::Auto, "auto"),
            (Cursor::Default, "default"),
            (Cursor::Pointer, "pointer"),
            (Cursor::Text, "text"),
            (Cursor::None, "none"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }
}
