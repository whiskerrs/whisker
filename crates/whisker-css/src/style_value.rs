//! Conversion from the compatibility authoring types to semantic style values.

use whisker_style::{
    CalcExpression, LengthPercentageValue, LengthUnit, LengthValue, StyleNumber, StyleValue,
};

use crate::{CalcExpr, CssString, Integer, Length, LengthPercentage, Number, Percentage};

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

    fn dimension(value: f32, unit: LengthUnit) -> LengthValue {
        LengthValue::Dimension {
            value: StyleNumber::new(value),
            unit,
        }
    }
}
