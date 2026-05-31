//! `<gradient>` — color transitions usable wherever Lynx accepts an
//! `<image>` (currently `background-image`, `mask-image`).
//!
//! Lynx reference: <https://lynxjs.org/api/css/data-type/gradient.html>
//!
//! Lynx supports three gradient functions:
//!
//! - [`Gradient::Linear`] — color stops along an arbitrary axis.
//! - [`Gradient::Radial`] — color stops radiating from a focal point.
//! - [`Gradient::Conic`] — color stops sweeping around an angle.
//!
//! **Repeating gradients (`repeating-linear-gradient`,
//! `repeating-radial-gradient`, `repeating-conic-gradient`) are not
//! supported by Lynx and are intentionally absent.** The
//! "multi-position color stop" shorthand (`red 40% 60%`) is also not
//! supported — express it as two separate stops at the matching
//! positions.

use core::fmt;

use crate::to_css::ToCss;

use super::{Angle, Color, LengthPercentage, Percentage};

/// A CSS `<gradient>` value.
#[derive(Clone, Debug, PartialEq)]
pub enum Gradient {
    /// `linear-gradient(<direction>, <stops>)`.
    Linear {
        /// Direction of the gradient axis.
        direction: LinearDirection,
        /// Color stops along the axis.
        stops: Vec<ColorStop>,
    },
    /// `radial-gradient(<shape>, <stops>)`.
    Radial {
        /// Shape and extent of the radial gradient.
        shape: RadialShape,
        /// Color stops along the radius.
        stops: Vec<ColorStop>,
    },
    /// `conic-gradient([from <angle>] [at <position>], <stops>)`.
    Conic {
        /// Starting angle of the sweep, if any.
        from: Option<Angle>,
        /// Center of the sweep as `<length-percentage> <length-percentage>`
        /// (defaults to `50% 50%` when `None`).
        at: Option<(LengthPercentage, LengthPercentage)>,
        /// Color stops along the sweep.
        stops: Vec<ColorStop>,
    },
}

impl Gradient {
    /// Convenience constructor for a vertical top-to-bottom linear
    /// gradient with the given color stops.
    pub fn linear_to_bottom(stops: impl IntoIterator<Item = ColorStop>) -> Self {
        Self::Linear {
            direction: LinearDirection::ToBottom,
            stops: stops.into_iter().collect(),
        }
    }

    /// Convenience constructor for a horizontal left-to-right linear
    /// gradient with the given color stops.
    pub fn linear_to_right(stops: impl IntoIterator<Item = ColorStop>) -> Self {
        Self::Linear {
            direction: LinearDirection::ToRight,
            stops: stops.into_iter().collect(),
        }
    }
}

/// The direction component of a `linear-gradient`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LinearDirection {
    /// `to top`.
    ToTop,
    /// `to right`.
    ToRight,
    /// `to bottom`.
    ToBottom,
    /// `to left`.
    ToLeft,
    /// `to top right`.
    ToTopRight,
    /// `to top left`.
    ToTopLeft,
    /// `to bottom right`.
    ToBottomRight,
    /// `to bottom left`.
    ToBottomLeft,
    /// Explicit angle (`<angle>`). 0deg points up; positive angles
    /// rotate clockwise.
    Angle(Angle),
}

impl ToCss for LinearDirection {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            LinearDirection::ToTop => dest.write_str("to top"),
            LinearDirection::ToRight => dest.write_str("to right"),
            LinearDirection::ToBottom => dest.write_str("to bottom"),
            LinearDirection::ToLeft => dest.write_str("to left"),
            LinearDirection::ToTopRight => dest.write_str("to top right"),
            LinearDirection::ToTopLeft => dest.write_str("to top left"),
            LinearDirection::ToBottomRight => dest.write_str("to bottom right"),
            LinearDirection::ToBottomLeft => dest.write_str("to bottom left"),
            LinearDirection::Angle(a) => a.to_css(dest),
        }
    }
}

/// The shape component of a `radial-gradient`.
#[derive(Clone, Debug, PartialEq)]
pub enum RadialShape {
    /// `circle` — equal radius along both axes.
    Circle,
    /// `ellipse` — independent horizontal and vertical radii.
    Ellipse,
    /// `circle <length>` — explicit circle radius.
    CircleSized(LengthPercentage),
    /// `ellipse <length-percentage> <length-percentage>` —
    /// explicit ellipse radii.
    EllipseSized(LengthPercentage, LengthPercentage),
}

impl ToCss for RadialShape {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            RadialShape::Circle => dest.write_str("circle"),
            RadialShape::Ellipse => dest.write_str("ellipse"),
            RadialShape::CircleSized(r) => {
                dest.write_str("circle ")?;
                r.to_css(dest)
            }
            RadialShape::EllipseSized(rx, ry) => {
                dest.write_str("ellipse ")?;
                rx.to_css(dest)?;
                dest.write_char(' ')?;
                ry.to_css(dest)
            }
        }
    }
}

/// A `<color-stop>`: a color and an optional position.
///
/// Lynx accepts the position as either a `<percentage>` or a
/// `<length>` (mapped via [`LengthPercentage`]). The
/// `<color> <position> <position>` "double-position" form is **not**
/// supported by Lynx; emit the same color twice if you need sharp
/// transitions.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorStop {
    /// Color of the stop.
    pub color: Color,
    /// Optional position along the gradient axis.
    pub position: Option<StopPosition>,
}

impl ColorStop {
    /// Color stop without an explicit position.
    pub fn new(color: Color) -> Self {
        Self {
            color,
            position: None,
        }
    }

    /// Color stop with an explicit position.
    pub fn at(color: Color, position: impl Into<StopPosition>) -> Self {
        Self {
            color,
            position: Some(position.into()),
        }
    }
}

impl ToCss for ColorStop {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        self.color.to_css(dest)?;
        if let Some(p) = &self.position {
            dest.write_char(' ')?;
            p.to_css(dest)?;
        }
        Ok(())
    }
}

/// Position of a [`ColorStop`].
#[derive(Clone, Debug, PartialEq)]
pub enum StopPosition {
    /// A length-percentage (`50%`, `100px`).
    LengthPercentage(LengthPercentage),
    /// A unit-less number, interpreted by Lynx as a fraction
    /// (`0` = start, `1` = end).
    Number(f32),
}

impl From<Percentage> for StopPosition {
    fn from(p: Percentage) -> Self {
        Self::LengthPercentage(p.into())
    }
}

impl From<LengthPercentage> for StopPosition {
    fn from(lp: LengthPercentage) -> Self {
        Self::LengthPercentage(lp)
    }
}

impl ToCss for StopPosition {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            StopPosition::LengthPercentage(lp) => lp.to_css(dest),
            StopPosition::Number(n) => crate::to_css::write_number(dest, *n),
        }
    }
}

fn write_stops(dest: &mut dyn fmt::Write, stops: &[ColorStop]) -> fmt::Result {
    let mut first = true;
    for s in stops {
        if !first {
            dest.write_str(", ")?;
        }
        s.to_css(dest)?;
        first = false;
    }
    Ok(())
}

impl ToCss for Gradient {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Gradient::Linear { direction, stops } => {
                dest.write_str("linear-gradient(")?;
                direction.to_css(dest)?;
                dest.write_str(", ")?;
                write_stops(dest, stops)?;
                dest.write_char(')')
            }
            Gradient::Radial { shape, stops } => {
                dest.write_str("radial-gradient(")?;
                shape.to_css(dest)?;
                dest.write_str(", ")?;
                write_stops(dest, stops)?;
                dest.write_char(')')
            }
            Gradient::Conic { from, at, stops } => {
                dest.write_str("conic-gradient(")?;
                let mut wrote_header = false;
                if let Some(a) = from {
                    dest.write_str("from ")?;
                    a.to_css(dest)?;
                    wrote_header = true;
                }
                if let Some((x, y)) = at {
                    if wrote_header {
                        dest.write_char(' ')?;
                    }
                    dest.write_str("at ")?;
                    x.to_css(dest)?;
                    dest.write_char(' ')?;
                    y.to_css(dest)?;
                    wrote_header = true;
                }
                if wrote_header {
                    dest.write_str(", ")?;
                }
                write_stops(dest, stops)?;
                dest.write_char(')')
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_type::{Length, NamedColor};

    fn red() -> Color {
        Color::Named(NamedColor::Red)
    }
    fn blue() -> Color {
        Color::Named(NamedColor::Blue)
    }

    #[test]
    fn linear_to_bottom_two_stops() {
        let g = Gradient::linear_to_bottom([ColorStop::new(red()), ColorStop::new(blue())]);
        assert_eq!(g.to_css_string(), "linear-gradient(to bottom, red, blue)");
    }

    #[test]
    fn linear_with_angle_and_positions() {
        let g = Gradient::Linear {
            direction: LinearDirection::Angle(Angle::Deg(45.0)),
            stops: vec![
                ColorStop::at(red(), Percentage(0.0)),
                ColorStop::at(blue(), Percentage(100.0)),
            ],
        };
        assert_eq!(
            g.to_css_string(),
            "linear-gradient(45deg, red 0%, blue 100%)"
        );
    }

    #[test]
    fn linear_all_keyword_directions() {
        let cases = [
            (LinearDirection::ToTop, "to top"),
            (LinearDirection::ToRight, "to right"),
            (LinearDirection::ToBottom, "to bottom"),
            (LinearDirection::ToLeft, "to left"),
            (LinearDirection::ToTopRight, "to top right"),
            (LinearDirection::ToTopLeft, "to top left"),
            (LinearDirection::ToBottomRight, "to bottom right"),
            (LinearDirection::ToBottomLeft, "to bottom left"),
        ];
        for (d, expected) in cases {
            let g = Gradient::Linear {
                direction: d,
                stops: vec![ColorStop::new(red())],
            };
            assert!(g.to_css_string().contains(expected));
        }
    }

    #[test]
    fn radial_circle_default() {
        let g = Gradient::Radial {
            shape: RadialShape::Circle,
            stops: vec![ColorStop::new(red()), ColorStop::new(blue())],
        };
        assert_eq!(g.to_css_string(), "radial-gradient(circle, red, blue)");
    }

    #[test]
    fn radial_ellipse_sized() {
        let g = Gradient::Radial {
            shape: RadialShape::EllipseSized(Length::Px(100.0).into(), Percentage(50.0).into()),
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(
            g.to_css_string(),
            "radial-gradient(ellipse 100px 50%, red)"
        );
    }

    #[test]
    fn radial_circle_sized() {
        let g = Gradient::Radial {
            shape: RadialShape::CircleSized(Length::Px(50.0).into()),
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(g.to_css_string(), "radial-gradient(circle 50px, red)");
    }

    #[test]
    fn radial_ellipse_keyword() {
        let g = Gradient::Radial {
            shape: RadialShape::Ellipse,
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(g.to_css_string(), "radial-gradient(ellipse, red)");
    }

    #[test]
    fn conic_bare() {
        let g = Gradient::Conic {
            from: None,
            at: None,
            stops: vec![ColorStop::new(red()), ColorStop::new(blue())],
        };
        assert_eq!(g.to_css_string(), "conic-gradient(red, blue)");
    }

    #[test]
    fn conic_from_and_at() {
        let g = Gradient::Conic {
            from: Some(Angle::Deg(90.0)),
            at: Some((Percentage(50.0).into(), Percentage(50.0).into())),
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(
            g.to_css_string(),
            "conic-gradient(from 90deg at 50% 50%, red)"
        );
    }

    #[test]
    fn conic_at_only() {
        let g = Gradient::Conic {
            from: None,
            at: Some((Percentage(0.0).into(), Percentage(100.0).into())),
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(g.to_css_string(), "conic-gradient(at 0% 100%, red)");
    }

    #[test]
    fn conic_from_only() {
        let g = Gradient::Conic {
            from: Some(Angle::Turn(0.25)),
            at: None,
            stops: vec![ColorStop::new(red())],
        };
        assert_eq!(g.to_css_string(), "conic-gradient(from 0.25turn, red)");
    }

    #[test]
    fn stop_with_number_position() {
        let stop = ColorStop {
            color: red(),
            position: Some(StopPosition::Number(0.5)),
        };
        assert_eq!(stop.to_css_string(), "red 0.5");
    }

    #[test]
    fn stop_position_from_impls() {
        let p: StopPosition = Percentage(25.0).into();
        let lp: StopPosition = LengthPercentage::Length(Length::Px(8.0)).into();
        assert_eq!(p.to_css_string(), "25%");
        assert_eq!(lp.to_css_string(), "8px");
    }

    #[test]
    fn linear_to_right_helper() {
        let g = Gradient::linear_to_right([ColorStop::new(red()), ColorStop::new(blue())]);
        assert_eq!(g.to_css_string(), "linear-gradient(to right, red, blue)");
    }
}
