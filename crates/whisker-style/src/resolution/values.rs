use super::*;

pub(super) fn canonical_features(values: &[FontFeatureValue]) -> Vec<FontFeatureValue> {
    values
        .iter()
        .fold(BTreeMap::new(), |mut result, value| {
            result.insert(value.tag, value.value);
            result
        })
        .into_iter()
        .map(|(tag, value)| FontFeatureValue { tag, value })
        .collect()
}

pub(super) fn canonical_variations(
    values: &[FontVariationValue],
) -> Result<Vec<FontVariationValue>, StyleResolutionError> {
    let mut result = BTreeMap::new();
    for value in values {
        if !value.value.get().is_finite() {
            return Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::FontVariationSettings,
            ));
        }
        result.insert(value.tag, value.value);
    }
    Ok(result
        .into_iter()
        .map(|(tag, value)| FontVariationValue { tag, value })
        .collect())
}

pub(super) fn expect_length_percentage(
    property: StyleProperty,
    value: &StyleValue,
) -> Result<&LengthPercentageValue, StyleResolutionError> {
    match value {
        StyleValue::LengthPercentage(value) => Ok(value),
        _ => Err(wrong_type(property)),
    }
}

pub(super) fn wrong_type(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

pub(super) fn resolved_component<T>(value: &ComponentValue<T>) -> &T {
    value
        .value()
        .expect("custom-property components are materialized before computed-style resolution")
}

pub(super) fn finite(
    value: StyleNumber,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    let value = value.get();
    if value.is_finite() {
        Ok(value)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

pub(super) fn resolve_length(
    value: LengthValue,
    em_basis: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    let (number, multiplier) = match value {
        LengthValue::Zero => return Ok(0.0),
        LengthValue::Dimension { value, unit } => {
            let multiplier = match unit {
                LengthUnit::Px => 1.0,
                LengthUnit::Em => em_basis,
                LengthUnit::Rem => environment.root_font_size(),
                LengthUnit::Vh => environment.viewport_height() / 100.0,
                LengthUnit::Vw => environment.viewport_width() / 100.0,
            };
            (finite(value, property)?, multiplier)
        }
    };
    let resolved = number * multiplier;
    if resolved.is_finite() {
        Ok(resolved)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

pub(super) fn resolve_length_percentage(
    value: &LengthPercentageValue,
    percentage_basis: f32,
    em_basis: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    let resolved = match value {
        LengthPercentageValue::Length(value) => {
            resolve_length(*value, em_basis, environment, property)?
        }
        LengthPercentageValue::Percentage(value) => {
            finite(*value, property)? * percentage_basis / 100.0
        }
        LengthPercentageValue::Calc(expression) => {
            match evaluate_calc(
                expression,
                percentage_basis,
                em_basis,
                environment,
                property,
            )? {
                Quantity::Length(value) => value,
                Quantity::Number(_) => {
                    return Err(StyleResolutionError::InvalidCalculation(property));
                }
            }
        }
    };
    if resolved.is_finite() {
        Ok(resolved)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Quantity {
    Number(f32),
    Length(f32),
}

pub(super) fn evaluate_calc(
    expression: &CalcExpression,
    percentage_basis: f32,
    em_basis: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<Quantity, StyleResolutionError> {
    let invalid = || StyleResolutionError::InvalidCalculation(property);
    match expression {
        CalcExpression::Value(value) => {
            resolve_length_percentage(value, percentage_basis, em_basis, environment, property)
                .map(Quantity::Length)
        }
        CalcExpression::Number(value) => finite(*value, property).map(Quantity::Number),
        CalcExpression::Variable(_) => Err(invalid()),
        CalcExpression::Add(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left + right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Length(left + right))
                }
                _ => Err(invalid()),
            }
        }
        CalcExpression::Sub(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left - right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Length(left - right))
                }
                _ => Err(invalid()),
            }
        }
        CalcExpression::Mul(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left * right))
                }
                (Quantity::Number(number), Quantity::Length(length))
                | (Quantity::Length(length), Quantity::Number(number)) => {
                    Ok(Quantity::Length(number * length))
                }
                (Quantity::Length(_), Quantity::Length(_)) => Err(invalid()),
            }
        }
        CalcExpression::Div(left, right) => {
            let left = evaluate_calc(left, percentage_basis, em_basis, environment, property)?;
            let right = evaluate_calc(right, percentage_basis, em_basis, environment, property)?;
            match (left, right) {
                (_, Quantity::Number(0.0)) | (_, Quantity::Length(0.0)) => Err(invalid()),
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left / right))
                }
                (Quantity::Length(left), Quantity::Number(right)) => {
                    Ok(Quantity::Length(left / right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Number(left / right))
                }
                (Quantity::Number(_), Quantity::Length(_)) => Err(invalid()),
            }
        }
    }
}

pub(super) fn normalize_color(value: &ColorValue) -> Result<ColorValue, StyleResolutionError> {
    normalize_color_for(value, StyleProperty::Color)
}

pub(crate) fn normalize_color_for(
    value: &ColorValue,
    property: StyleProperty,
) -> Result<ColorValue, StyleResolutionError> {
    let invalid = || StyleResolutionError::InvalidPropertyValue(property);
    match value {
        ColorValue::Named(name) if name.is_empty() => Err(invalid()),
        ColorValue::Named(name) => Ok(ColorValue::Named(name.clone())),
        ColorValue::Rgba {
            red,
            green,
            blue,
            alpha,
        } => {
            let alpha = finite(*alpha, property)?;
            if !(0.0..=1.0).contains(&alpha) {
                return Err(invalid());
            }
            Ok(ColorValue::Rgba {
                red: *red,
                green: *green,
                blue: *blue,
                alpha: StyleNumber::new(alpha),
            })
        }
        ColorValue::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            let hue = finite(*hue_degrees, property)?.rem_euclid(360.0);
            let saturation = finite(*saturation, property)?;
            let lightness = finite(*lightness, property)?;
            let alpha = finite(*alpha, property)?;
            if !(0.0..=100.0).contains(&saturation)
                || !(0.0..=100.0).contains(&lightness)
                || !(0.0..=1.0).contains(&alpha)
            {
                return Err(invalid());
            }
            Ok(ColorValue::Hsla {
                hue_degrees: StyleNumber::new(hue),
                saturation: StyleNumber::new(saturation),
                lightness: StyleNumber::new(lightness),
                alpha: StyleNumber::new(alpha),
            })
        }
    }
}
