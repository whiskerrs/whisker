//! Typed `var()` references used inside property values and functions.

use core::fmt;

use whisker_style::{ComponentValue, CustomPropertyName, CustomPropertyReference, StyleValue};

use crate::{ToCss, style_value::ToStyleValue};

/// A literal typed value or a reference to an inherited custom property.
///
/// Unlike browser token streams, the type parameter records the grammar slot
/// in which the value is used. Resolution still follows CSS missing-value,
/// fallback, inheritance, and cycle semantics.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueOrVariable<T> {
    /// A literal typed value.
    Value(T),
    /// `var(--name)` with an optional typed fallback.
    Variable {
        /// Case-sensitive custom-property name.
        name: CustomPropertyName,
        /// Fallback used when the referenced value is missing or cyclic.
        fallback: Option<Box<T>>,
    },
}

impl<T> ValueOrVariable<T> {
    /// Creates `var(--name)`.
    pub fn variable(name: CustomPropertyName) -> Self {
        Self::Variable {
            name,
            fallback: None,
        }
    }

    /// Creates `var(--name, <fallback>)`.
    pub fn variable_with_fallback(name: CustomPropertyName, fallback: T) -> Self {
        Self::Variable {
            name,
            fallback: Some(Box::new(fallback)),
        }
    }

    pub(crate) fn to_component<U>(&self, literal: impl FnOnce(&T) -> U) -> ComponentValue<U>
    where
        T: ToStyleValue,
    {
        match self {
            Self::Value(value) => ComponentValue::Value(literal(value)),
            Self::Variable { name, fallback } => {
                let reference = fallback.as_deref().map_or_else(
                    || CustomPropertyReference::new(name.clone()),
                    |fallback| {
                        CustomPropertyReference::with_fallback(
                            name.clone(),
                            fallback.to_style_value(),
                        )
                    },
                );
                ComponentValue::Variable(reference)
            }
        }
    }
}

impl<T> From<T> for ValueOrVariable<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

impl From<f32> for ValueOrVariable<crate::Number> {
    fn from(value: f32) -> Self {
        Self::Value(value.into())
    }
}

impl From<i32> for ValueOrVariable<crate::Number> {
    fn from(value: i32) -> Self {
        Self::Value(value.into())
    }
}

impl<T: ToCss> ToCss for ValueOrVariable<T> {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        match self {
            Self::Value(value) => value.to_css(dest),
            Self::Variable { name, fallback } => {
                dest.write_str("var(")?;
                dest.write_str(name.as_str())?;
                if let Some(fallback) = fallback {
                    dest.write_str(", ")?;
                    fallback.to_css(dest)?;
                }
                dest.write_char(')')
            }
        }
    }
}

impl<T> ToStyleValue for ValueOrVariable<T>
where
    T: ToStyleValue,
{
    fn to_style_value(&self) -> StyleValue {
        match self {
            Self::Value(value) => value.to_style_value(),
            Self::Variable { name, fallback } => {
                StyleValue::Variable(fallback.as_deref().map_or_else(
                    || CustomPropertyReference::new(name.clone()),
                    |fallback| {
                        CustomPropertyReference::with_fallback(
                            name.clone(),
                            fallback.to_style_value(),
                        )
                    },
                ))
            }
        }
    }
}

/// Creates a typed `var(--name)` reference.
pub fn custom_var<T>(name: CustomPropertyName) -> ValueOrVariable<T> {
    ValueOrVariable::variable(name)
}

/// Creates a typed `var(--name, <fallback>)` reference.
pub fn custom_var_with_fallback<T>(name: CustomPropertyName, fallback: T) -> ValueOrVariable<T> {
    ValueOrVariable::variable_with_fallback(name, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Angle, Color, NamedColor};

    #[test]
    fn typed_variables_serialize_with_and_without_fallbacks() {
        let name = CustomPropertyName::new("--accent").unwrap();
        assert_eq!(
            custom_var::<Color>(name.clone()).to_css_string(),
            "var(--accent)"
        );
        assert_eq!(
            custom_var_with_fallback(name, Color::Named(NamedColor::Red)).to_css_string(),
            "var(--accent, red)"
        );
    }

    #[test]
    fn literal_values_keep_their_normal_serialization() {
        assert_eq!(
            ValueOrVariable::from(Angle::Deg(45.0)).to_css_string(),
            "45deg"
        );
    }
}
