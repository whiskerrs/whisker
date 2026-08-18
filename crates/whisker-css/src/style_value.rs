//! Conversion from the compatibility authoring types to semantic style values.

use whisker_style::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, BoxSizingValue, CalcExpression, ColorValue,
    DirectionValue, DisplayValue, FlexBasisValue, FlexDirectionValue, FlexWrapValue,
    FontStyleValue, FontWeightValue, JustifyContentValue, LengthPercentageAutoValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, PositionValue, SizeValue,
    StyleNumber, StyleValue,
};

use crate::{
    AlignContent, AlignItems, AlignSelf, Angle, BoxSizing, CalcExpr, Color, CssString, Direction,
    Display, FlexBasis, FlexDirection, FlexWrap, FontStyle, FontWeight, Integer, JustifyContent,
    Length, LengthPercentage, LineHeight, MarginValue, Number, Percentage, PositionKind, Size,
};

pub(crate) trait ToStyleValue {
    fn to_style_value(&self) -> StyleValue;
}

impl ToStyleValue for Length {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Length(to_length(*self))
    }
}

impl ToStyleValue for Percentage {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentage(LengthPercentageValue::Percentage(StyleNumber::new(self.0)))
    }
}

impl ToStyleValue for LengthPercentage {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentage(to_length_percentage(self))
    }
}

impl ToStyleValue for Number {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Number(StyleNumber::new(self.0))
    }
}

impl ToStyleValue for Integer {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Integer(i64::from(self.0))
    }
}

impl ToStyleValue for CssString {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Text(self.0.clone())
    }
}

impl ToStyleValue for FontStyle {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FontStyle(match self {
            Self::Normal => FontStyleValue::Normal,
            Self::Italic => FontStyleValue::Italic,
            Self::Oblique => FontStyleValue::Oblique,
        })
    }
}

impl ToStyleValue for FontWeight {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Normal => FontWeightValue::NORMAL,
            Self::Bold => FontWeightValue::BOLD,
            Self::Numeric(value) => FontWeightValue::from_raw(*value),
        };
        StyleValue::FontWeight(value)
    }
}

impl ToStyleValue for LineHeight {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Normal => LineHeightValue::Normal,
            Self::Number(value) => LineHeightValue::Number(StyleNumber::new(*value)),
            Self::LengthPercentage(value) => {
                LineHeightValue::LengthPercentage(to_length_percentage(value))
            }
        };
        StyleValue::LineHeight(value)
    }
}

impl ToStyleValue for Color {
    fn to_style_value(&self) -> StyleValue {
        let value = match self {
            Self::Named(value) => ColorValue::Named(value.name().into()),
            Self::Transparent => ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: StyleNumber::new(0.0),
            },
            Self::Rgba(red, green, blue, alpha) => ColorValue::Rgba {
                red: *red,
                green: *green,
                blue: *blue,
                alpha: StyleNumber::new(*alpha),
            },
            Self::Hsla { h, s, l, a } => ColorValue::Hsla {
                hue_degrees: StyleNumber::new(angle_degrees(*h)),
                saturation: StyleNumber::new(*s),
                lightness: StyleNumber::new(*l),
                alpha: StyleNumber::new(*a),
            },
        };
        StyleValue::Color(value)
    }
}

impl ToStyleValue for Display {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Display(match self {
            Self::None => DisplayValue::None,
            Self::Flex => DisplayValue::Flex,
            Self::Grid => DisplayValue::Grid,
            Self::Linear => DisplayValue::Linear,
            Self::Relative => DisplayValue::Relative,
        })
    }
}

impl ToStyleValue for PositionKind {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Position(match self {
            Self::Relative => PositionValue::Relative,
            Self::Absolute => PositionValue::Absolute,
            Self::Fixed => PositionValue::Fixed,
            Self::Sticky => PositionValue::Sticky,
        })
    }
}

impl ToStyleValue for BoxSizing {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::BoxSizing(match self {
            Self::ContentBox => BoxSizingValue::ContentBox,
            Self::BorderBox => BoxSizingValue::BorderBox,
        })
    }
}

impl ToStyleValue for Direction {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Direction(match self {
            Self::Ltr => DirectionValue::Ltr,
            Self::Rtl => DirectionValue::Rtl,
        })
    }
}

impl ToStyleValue for Size {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::Size(match self {
            Self::Auto => SizeValue::Auto,
            Self::LengthPercentage(value) => {
                SizeValue::LengthPercentage(to_length_percentage(value))
            }
            Self::MaxContent => SizeValue::MaxContent,
            Self::MinContent => SizeValue::MinContent,
            Self::FitContent(value) => {
                SizeValue::FitContent(value.0.as_ref().map(to_length_percentage))
            }
            Self::None => SizeValue::None,
        })
    }
}

impl ToStyleValue for MarginValue {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::LengthPercentageAuto(match self {
            Self::Auto => LengthPercentageAutoValue::Auto,
            Self::LengthPercentage(value) => {
                LengthPercentageAutoValue::LengthPercentage(to_length_percentage(value))
            }
        })
    }
}

impl ToStyleValue for FlexDirection {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexDirection(match self {
            Self::Row => FlexDirectionValue::Row,
            Self::RowReverse => FlexDirectionValue::RowReverse,
            Self::Column => FlexDirectionValue::Column,
            Self::ColumnReverse => FlexDirectionValue::ColumnReverse,
        })
    }
}

impl ToStyleValue for FlexWrap {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexWrap(match self {
            Self::Nowrap => FlexWrapValue::NoWrap,
            Self::Wrap => FlexWrapValue::Wrap,
            Self::WrapReverse => FlexWrapValue::WrapReverse,
        })
    }
}

impl ToStyleValue for FlexBasis {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::FlexBasis(match self {
            Self::Auto => FlexBasisValue::Auto,
            Self::Content => FlexBasisValue::Content,
            Self::LengthPercentage(value) => {
                FlexBasisValue::LengthPercentage(to_length_percentage(value))
            }
        })
    }
}

impl ToStyleValue for JustifyContent {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::JustifyContent(match self {
            Self::Stretch => JustifyContentValue::Stretch,
            Self::FlexStart => JustifyContentValue::FlexStart,
            Self::FlexEnd => JustifyContentValue::FlexEnd,
            Self::Center => JustifyContentValue::Center,
            Self::SpaceBetween => JustifyContentValue::SpaceBetween,
            Self::SpaceAround => JustifyContentValue::SpaceAround,
            Self::SpaceEvenly => JustifyContentValue::SpaceEvenly,
            Self::Start => JustifyContentValue::Start,
            Self::End => JustifyContentValue::End,
        })
    }
}

impl ToStyleValue for AlignItems {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignItems(match self {
            Self::Stretch => AlignItemsValue::Stretch,
            Self::FlexStart => AlignItemsValue::FlexStart,
            Self::FlexEnd => AlignItemsValue::FlexEnd,
            Self::Center => AlignItemsValue::Center,
            Self::Baseline => AlignItemsValue::Baseline,
            Self::Start => AlignItemsValue::Start,
            Self::End => AlignItemsValue::End,
        })
    }
}

impl ToStyleValue for AlignSelf {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignSelf(match self {
            Self::Auto => AlignSelfValue::Auto,
            Self::Stretch => AlignSelfValue::Stretch,
            Self::FlexStart => AlignSelfValue::FlexStart,
            Self::FlexEnd => AlignSelfValue::FlexEnd,
            Self::Center => AlignSelfValue::Center,
            Self::Baseline => AlignSelfValue::Baseline,
            Self::Start => AlignSelfValue::Start,
            Self::End => AlignSelfValue::End,
        })
    }
}

impl ToStyleValue for AlignContent {
    fn to_style_value(&self) -> StyleValue {
        StyleValue::AlignContent(match self {
            Self::Stretch => AlignContentValue::Stretch,
            Self::FlexStart => AlignContentValue::FlexStart,
            Self::FlexEnd => AlignContentValue::FlexEnd,
            Self::Center => AlignContentValue::Center,
            Self::SpaceBetween => AlignContentValue::SpaceBetween,
            Self::SpaceAround => AlignContentValue::SpaceAround,
            Self::SpaceEvenly => AlignContentValue::SpaceEvenly,
        })
    }
}

fn angle_degrees(value: Angle) -> f32 {
    match value {
        Angle::Deg(value) => value,
        Angle::Rad(value) => value.to_degrees(),
        Angle::Turn(value) => value * 360.0,
    }
}

fn to_length(value: Length) -> LengthValue {
    let (value, unit) = match value {
        Length::Zero => return LengthValue::Zero,
        Length::Px(value) => (value, LengthUnit::Px),
        Length::Rpx(value) => (value, LengthUnit::Rpx),
        Length::Ppx(value) => (value, LengthUnit::Ppx),
        Length::Em(value) => (value, LengthUnit::Em),
        Length::Rem(value) => (value, LengthUnit::Rem),
        Length::Vh(value) => (value, LengthUnit::Vh),
        Length::Vw(value) => (value, LengthUnit::Vw),
    };
    LengthValue::Dimension {
        value: StyleNumber::new(value),
        unit,
    }
}

fn to_length_percentage(value: &LengthPercentage) -> LengthPercentageValue {
    match value {
        LengthPercentage::Length(value) => LengthPercentageValue::Length(to_length(*value)),
        LengthPercentage::Percentage(value) => {
            LengthPercentageValue::Percentage(StyleNumber::new(value.0))
        }
        LengthPercentage::Calc(value) => {
            LengthPercentageValue::Calc(Box::new(to_calc_expression(value)))
        }
    }
}

fn to_calc_expression(value: &CalcExpr) -> CalcExpression {
    match value {
        CalcExpr::Value(value) => CalcExpression::Value(Box::new(to_length_percentage(value))),
        CalcExpr::Number(value) => CalcExpression::Number(StyleNumber::new(*value)),
        CalcExpr::Add(left, right) => CalcExpression::Add(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Sub(left, right) => CalcExpression::Sub(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Mul(left, right) => CalcExpression::Mul(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
        CalcExpr::Div(left, right) => CalcExpression::Div(
            Box::new(to_calc_expression(left)),
            Box::new(to_calc_expression(right)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_length_unit_converts_semantically() {
        let cases = [
            (Length::Zero, LengthValue::Zero),
            (Length::Px(1.0), dimension(1.0, LengthUnit::Px)),
            (Length::Rpx(2.0), dimension(2.0, LengthUnit::Rpx)),
            (Length::Ppx(3.0), dimension(3.0, LengthUnit::Ppx)),
            (Length::Em(4.0), dimension(4.0, LengthUnit::Em)),
            (Length::Rem(5.0), dimension(5.0, LengthUnit::Rem)),
            (Length::Vh(6.0), dimension(6.0, LengthUnit::Vh)),
            (Length::Vw(7.0), dimension(7.0, LengthUnit::Vw)),
        ];
        for (input, expected) in cases {
            assert_eq!(input.to_style_value(), StyleValue::Length(expected));
        }
    }

    #[test]
    fn scalar_authoring_types_keep_semantics() {
        assert_eq!(
            Number(1.5).to_style_value(),
            StyleValue::Number(StyleNumber::new(1.5))
        );
        assert_eq!(Integer(-2).to_style_value(), StyleValue::Integer(-2));
        assert_eq!(
            CssString::new("hello").to_style_value(),
            StyleValue::Text("hello".into())
        );
        assert_eq!(
            Percentage(25.0).to_style_value(),
            StyleValue::LengthPercentage(LengthPercentageValue::Percentage(StyleNumber::new(25.0)))
        );
    }

    #[test]
    fn every_calc_operator_converts_as_a_tree() {
        let leaf = || CalcExpr::value(Length::Px(1.0));
        for expression in [
            leaf().add(leaf()),
            leaf().sub(leaf()),
            leaf().mul(CalcExpr::number(2.0)),
            leaf().div(CalcExpr::number(2.0)),
        ] {
            let value = LengthPercentage::calc(expression).to_style_value();
            assert!(matches!(
                value,
                StyleValue::LengthPercentage(LengthPercentageValue::Calc(_))
            ));
        }
    }

    #[test]
    fn inherited_authoring_values_convert_without_css_parsing() {
        for (input, expected) in [
            (FontStyle::Normal, FontStyleValue::Normal),
            (FontStyle::Italic, FontStyleValue::Italic),
            (FontStyle::Oblique, FontStyleValue::Oblique),
        ] {
            assert_eq!(input.to_style_value(), StyleValue::FontStyle(expected));
        }
        for (input, expected) in [
            (FontWeight::Normal, FontWeightValue::NORMAL),
            (FontWeight::Bold, FontWeightValue::BOLD),
            (FontWeight::Numeric(650), FontWeightValue::from_raw(650)),
        ] {
            assert_eq!(input.to_style_value(), StyleValue::FontWeight(expected));
        }
        assert_eq!(
            LineHeight::Normal.to_style_value(),
            StyleValue::LineHeight(LineHeightValue::Normal)
        );
        assert_eq!(
            LineHeight::Number(1.5).to_style_value(),
            StyleValue::LineHeight(LineHeightValue::Number(StyleNumber::new(1.5)))
        );
        assert_eq!(
            LineHeight::LengthPercentage(Length::Px(20.0).into()).to_style_value(),
            StyleValue::LineHeight(LineHeightValue::LengthPercentage(
                LengthPercentageValue::Length(dimension(20.0, LengthUnit::Px))
            ))
        );
    }

    #[test]
    fn every_color_form_becomes_a_typed_color_value() {
        assert_eq!(
            Color::Named(crate::NamedColor::Red).to_style_value(),
            StyleValue::Color(ColorValue::Named("red".into()))
        );
        assert_eq!(
            Color::Transparent.to_style_value(),
            StyleValue::Color(ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: StyleNumber::new(0.0),
            })
        );
        assert_eq!(
            Color::rgba(1, 2, 3, 0.5).to_style_value(),
            StyleValue::Color(ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: StyleNumber::new(0.5),
            })
        );
        for (angle, degrees) in [
            (Angle::Deg(90.0), 90.0),
            (Angle::Rad(core::f32::consts::FRAC_PI_2), 90.0),
            (Angle::Turn(0.25), 90.0),
        ] {
            assert_eq!(
                Color::Hsla {
                    h: angle,
                    s: 50.0,
                    l: 25.0,
                    a: 0.75,
                }
                .to_style_value(),
                StyleValue::Color(ColorValue::Hsla {
                    hue_degrees: StyleNumber::new(degrees),
                    saturation: StyleNumber::new(50.0),
                    lightness: StyleNumber::new(25.0),
                    alpha: StyleNumber::new(0.75),
                })
            );
        }
    }

    fn dimension(value: f32, unit: LengthUnit) -> LengthValue {
        LengthValue::Dimension {
            value: StyleNumber::new(value),
            unit,
        }
    }
}
