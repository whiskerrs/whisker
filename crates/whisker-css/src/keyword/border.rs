//! Border-related keyword enums.
//!
//! References:
//! - <https://lynxjs.org/api/css/properties/border-style>

use core::fmt;

use crate::to_css::ToCss;

/// The `border-style` keyword. Lynx accepts the full CSS set of
/// border styles for the four sides.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    /// `none` — no border.
    None,
    /// `hidden` — no border, but takes priority for border-conflict
    /// resolution in tables.
    Hidden,
    /// `solid` — a single solid line.
    Solid,
    /// `dashed` — a series of short dashes.
    Dashed,
    /// `dotted` — a series of dots.
    Dotted,
    /// `double` — two parallel lines with a gap.
    Double,
    /// `groove` — a 3-D grooved appearance.
    Groove,
    /// `ridge` — a 3-D ridged appearance.
    Ridge,
    /// `inset` — a 3-D inset appearance.
    Inset,
    /// `outset` — a 3-D outset appearance.
    Outset,
}

impl ToCss for BorderStyle {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            BorderStyle::None => "none",
            BorderStyle::Hidden => "hidden",
            BorderStyle::Solid => "solid",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
            BorderStyle::Double => "double",
            BorderStyle::Groove => "groove",
            BorderStyle::Ridge => "ridge",
            BorderStyle::Inset => "inset",
            BorderStyle::Outset => "outset",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_border_styles() {
        let cases = [
            (BorderStyle::None, "none"),
            (BorderStyle::Hidden, "hidden"),
            (BorderStyle::Solid, "solid"),
            (BorderStyle::Dashed, "dashed"),
            (BorderStyle::Dotted, "dotted"),
            (BorderStyle::Double, "double"),
            (BorderStyle::Groove, "groove"),
            (BorderStyle::Ridge, "ridge"),
            (BorderStyle::Inset, "inset"),
            (BorderStyle::Outset, "outset"),
        ];
        for (k, expected) in cases {
            assert_eq!(k.to_css_string(), expected);
        }
    }
}
