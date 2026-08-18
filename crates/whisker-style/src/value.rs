//! Renderer-independent values stored in specified style declarations.

/// An `f32` with equality and hashing defined by its IEEE-754 bit pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleNumber(u32);

impl StyleNumber {
    /// Stores a number without changing its representation.
    pub const fn new(value: f32) -> Self {
        Self(value.to_bits())
    }

    /// Returns the stored number.
    pub const fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// A unit accepted by Whisker's length model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    /// Logical pixels: iOS points and Android density-independent pixels.
    Px,
    /// Responsive pixels relative to a 750-unit viewport width.
    Rpx,
    /// Physical device pixels.
    Ppx,
    /// Units relative to the element's computed font size.
    Em,
    /// Units relative to the root computed font size.
    Rem,
    /// Percent of viewport height.
    Vh,
    /// Percent of viewport width.
    Vw,
}

/// A semantic length that does not require CSS parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthValue {
    /// Unitless zero.
    Zero,
    /// A number paired with an explicit unit.
    Dimension {
        /// Numeric magnitude.
        value: StyleNumber,
        /// Length unit.
        unit: LengthUnit,
    },
}

/// A semantic length-or-percentage value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LengthPercentageValue {
    /// Absolute or environment-relative length.
    Length(LengthValue),
    /// Percentage number before the `%` suffix.
    Percentage(StyleNumber),
    /// Typed arithmetic expression.
    Calc(Box<CalcExpression>),
}

/// A typed arithmetic expression used by length-percentage values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CalcExpression {
    /// Length-percentage operand.
    Value(Box<LengthPercentageValue>),
    /// Unitless numeric operand.
    Number(StyleNumber),
    /// Addition.
    Add(Box<Self>, Box<Self>),
    /// Subtraction.
    Sub(Box<Self>, Box<Self>),
    /// Multiplication.
    Mul(Box<Self>, Box<Self>),
    /// Division.
    Div(Box<Self>, Box<Self>),
}

/// An owned value in a specified inline-style declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StyleValue {
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Integer(i64),
    /// Unitless real number.
    Number(StyleNumber),
    /// UTF-8 text whose interpretation is defined by its property schema.
    Text(String),
    /// Length value.
    Length(LengthValue),
    /// Length, percentage, or typed `calc` expression.
    LengthPercentage(LengthPercentageValue),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn style_number_preserves_bits_and_hashes() {
        let number = StyleNumber::new(-0.0);
        assert_eq!(number.get().to_bits(), (-0.0_f32).to_bits());
        let mut values = HashSet::new();
        assert!(values.insert(number));
        assert!(!values.insert(number));
    }

    #[test]
    fn semantic_values_clone_and_compare_without_text() {
        let length = LengthValue::Dimension {
            value: StyleNumber::new(12.5),
            unit: LengthUnit::Px,
        };
        let calc = CalcExpression::Add(
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Length(length),
            ))),
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Percentage(StyleNumber::new(50.0)),
            ))),
        );
        let value = StyleValue::LengthPercentage(LengthPercentageValue::Calc(Box::new(calc)));
        assert_eq!(value.clone(), value);
        assert_ne!(value, StyleValue::Text("calc(12.5px + 50%)".into()));
    }

    #[test]
    fn scalar_variants_remain_distinct() {
        assert_ne!(StyleValue::Bool(true), StyleValue::Integer(1));
        assert_ne!(
            StyleValue::Number(StyleNumber::new(1.0)),
            StyleValue::Text("1".into())
        );
        assert_eq!(
            StyleValue::Length(LengthValue::Zero),
            StyleValue::Length(LengthValue::Zero)
        );
    }
}
