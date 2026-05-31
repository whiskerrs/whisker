//! `<position>` — a 2-D coordinate used by `background-position`,
//! `transform-origin`, and the `at` argument of radial and conic
//! gradients.
//!
//! Lynx does not document `<position>` as a standalone data type;
//! its grammar is described inline on each consuming property page.
//! The Whisker representation accepts either a paired
//! `<length-percentage>` and an optional axis-keyword pair (`top`,
//! `right`, `bottom`, `left`, `center`).

use core::fmt;

use crate::to_css::ToCss;

use super::super::data_type::LengthPercentage;

/// A `<position>` value.
#[derive(Clone, Debug, PartialEq)]
pub enum Position {
    /// A single keyword (`center`, `top`, …) expanded by Lynx to
    /// `<keyword> center` or `center <keyword>` depending on axis.
    Keyword(PositionKeyword),
    /// Two keywords, one per axis.
    Keywords(PositionKeyword, PositionKeyword),
    /// Two length-percentages: horizontal then vertical.
    Coords(LengthPercentage, LengthPercentage),
    /// A keyword followed by a length-percentage offset on the
    /// opposite axis (`top 10px`).
    Mixed(PositionKeyword, LengthPercentage),
}

/// Axis keyword used by [`Position`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PositionKeyword {
    /// Left edge.
    Left,
    /// Right edge.
    Right,
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
    /// Centered along the axis.
    Center,
}

impl ToCss for PositionKeyword {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(match self {
            PositionKeyword::Left => "left",
            PositionKeyword::Right => "right",
            PositionKeyword::Top => "top",
            PositionKeyword::Bottom => "bottom",
            PositionKeyword::Center => "center",
        })
    }
}

impl ToCss for Position {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Position::Keyword(k) => k.to_css(dest),
            Position::Keywords(a, b) => {
                a.to_css(dest)?;
                dest.write_char(' ')?;
                b.to_css(dest)
            }
            Position::Coords(x, y) => {
                x.to_css(dest)?;
                dest.write_char(' ')?;
                y.to_css(dest)
            }
            Position::Mixed(k, offset) => {
                k.to_css(dest)?;
                dest.write_char(' ')?;
                offset.to_css(dest)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_type::{Length, Percentage};

    #[test]
    fn single_keyword() {
        assert_eq!(
            Position::Keyword(PositionKeyword::Center).to_css_string(),
            "center"
        );
    }

    #[test]
    fn two_keywords() {
        let p = Position::Keywords(PositionKeyword::Top, PositionKeyword::Right);
        assert_eq!(p.to_css_string(), "top right");
    }

    #[test]
    fn coords() {
        let p = Position::Coords(Percentage(50.0).into(), Length::Px(10.0).into());
        assert_eq!(p.to_css_string(), "50% 10px");
    }

    #[test]
    fn mixed_keyword_and_offset() {
        let p = Position::Mixed(PositionKeyword::Top, Length::Px(10.0).into());
        assert_eq!(p.to_css_string(), "top 10px");
    }

    #[test]
    fn all_keywords() {
        let cases = [
            (PositionKeyword::Left, "left"),
            (PositionKeyword::Right, "right"),
            (PositionKeyword::Top, "top"),
            (PositionKeyword::Bottom, "bottom"),
            (PositionKeyword::Center, "center"),
        ];
        for (k, expected) in cases {
            assert_eq!(Position::Keyword(k).to_css_string(), expected);
        }
    }
}
