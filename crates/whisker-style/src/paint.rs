//! Computed paint values that remain independent of every Host renderer.

use crate::{
    ColorValue, ComputedLengthPercentage, Edges, InheritedStyle, SpecifiedStyle, StyleEnvironment,
    StyleNumber, StyleProperty, StyleResolutionError, StyleValue, layout::resolve_affine,
};

/// Four physical corners in top-left, top-right, bottom-right, bottom-left order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Corners<T> {
    /// Top-left corner.
    pub top_left: T,
    /// Top-right corner.
    pub top_right: T,
    /// Bottom-right corner.
    pub bottom_right: T,
    /// Bottom-left corner.
    pub bottom_left: T,
}

/// A computed border radius retaining both percentage axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedCornerRadius {
    /// Horizontal radius, resolved against border-box width by the renderer.
    pub horizontal: ComputedLengthPercentage,
    /// Vertical radius, resolved against border-box height by the renderer.
    pub vertical: ComputedLengthPercentage,
}

impl ComputedCornerRadius {
    const ZERO: Self = Self {
        horizontal: ComputedLengthPercentage::ZERO,
        vertical: ComputedLengthPercentage::ZERO,
    };
}

impl<T: Copy> Corners<T> {
    const fn all(value: T) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_right: value,
            bottom_left: value,
        }
    }
}

/// Renderer-independent border line style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyleValue {
    /// No border is painted.
    #[default]
    None,
    /// Hidden border, equivalent to none outside table conflict resolution.
    Hidden,
    /// One solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
    /// Two parallel lines.
    Double,
    /// Grooved 3-D line.
    Groove,
    /// Ridged 3-D line.
    Ridge,
    /// Inset 3-D line.
    Inset,
    /// Outset 3-D line.
    Outset,
}

/// Whether overflow on one axis is visible or clipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverflowValue {
    /// Descendant paint may extend outside the box.
    #[default]
    Visible,
    /// Descendant paint is clipped to the box.
    Hidden,
}

/// Whether a node participates in painting while retaining layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VisibilityValue {
    /// Paint the node normally.
    #[default]
    Visible,
    /// Do not paint the node, but retain its layout box.
    Hidden,
}

/// Computed background, border, clip, and compositing values for one node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedPaintStyle {
    /// Resolved background color. Transparent is represented explicitly.
    pub background_color: ColorValue,
    /// Resolved border colors in physical edge order.
    pub border_colors: Edges<ColorValue>,
    /// Border line styles in physical edge order.
    pub border_styles: Edges<BorderStyleValue>,
    /// Corner radii retaining their border-box percentage component.
    pub border_radii: Corners<ComputedCornerRadius>,
    /// Group opacity, clamped to `0.0..=1.0`.
    pub opacity: StyleNumber,
    /// Paint visibility.
    pub visibility: VisibilityValue,
    /// Horizontal overflow behavior.
    pub overflow_x: OverflowValue,
    /// Vertical overflow behavior.
    pub overflow_y: OverflowValue,
    /// Sibling stacking key.
    pub z_index: i32,
}

impl ComputedPaintStyle {
    fn initial(current_color: &ColorValue) -> Self {
        let transparent = ColorValue::Rgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: StyleNumber::new(0.0),
        };
        Self {
            background_color: transparent,
            border_colors: Edges {
                top: current_color.clone(),
                right: current_color.clone(),
                bottom: current_color.clone(),
                left: current_color.clone(),
            },
            border_styles: Edges {
                top: BorderStyleValue::None,
                right: BorderStyleValue::None,
                bottom: BorderStyleValue::None,
                left: BorderStyleValue::None,
            },
            border_radii: Corners::all(ComputedCornerRadius::ZERO),
            opacity: StyleNumber::new(1.0),
            visibility: VisibilityValue::Visible,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Visible,
            z_index: 0,
        }
    }

    /// Returns paint invalidation when any computed presentation value changed.
    pub fn changes_from(&self, previous: &Self) -> crate::PropertyImpactSet {
        if self == previous {
            crate::PropertyImpactSet::EMPTY
        } else {
            crate::PropertyImpactSet::PAINT
        }
    }
}

pub(crate) fn resolve_paint_style(
    specified: &SpecifiedStyle,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
) -> Result<ComputedPaintStyle, StyleResolutionError> {
    let mut paint = ComputedPaintStyle::initial(inherited.color());
    for declaration in specified.resolved() {
        let property = declaration.property();
        let value = declaration.value();
        match property {
            StyleProperty::BackgroundColor => {
                paint.background_color = color(value, property)?;
            }
            StyleProperty::BorderTopColor => paint.border_colors.top = color(value, property)?,
            StyleProperty::BorderRightColor => paint.border_colors.right = color(value, property)?,
            StyleProperty::BorderBottomColor => {
                paint.border_colors.bottom = color(value, property)?;
            }
            StyleProperty::BorderLeftColor => paint.border_colors.left = color(value, property)?,
            StyleProperty::BorderTopStyle => {
                paint.border_styles.top = border_style(value, property)?
            }
            StyleProperty::BorderRightStyle => {
                paint.border_styles.right = border_style(value, property)?;
            }
            StyleProperty::BorderBottomStyle => {
                paint.border_styles.bottom = border_style(value, property)?;
            }
            StyleProperty::BorderLeftStyle => {
                paint.border_styles.left = border_style(value, property)?;
            }
            StyleProperty::BorderTopLeftRadius => {
                paint.border_radii.top_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderTopRightRadius => {
                paint.border_radii.top_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderBottomRightRadius => {
                paint.border_radii.bottom_right = radius(value, inherited, environment, property)?;
            }
            StyleProperty::BorderBottomLeftRadius => {
                paint.border_radii.bottom_left = radius(value, inherited, environment, property)?;
            }
            StyleProperty::Opacity => {
                let StyleValue::Number(value) = value else {
                    return Err(invalid(property));
                };
                let value = value.get();
                if !value.is_finite() {
                    return Err(invalid(property));
                }
                paint.opacity = StyleNumber::new(value.clamp(0.0, 1.0));
            }
            StyleProperty::Visibility => {
                let StyleValue::Visibility(value) = value else {
                    return Err(invalid(property));
                };
                paint.visibility = *value;
            }
            StyleProperty::OverflowX => {
                let StyleValue::Overflow(value) = value else {
                    return Err(invalid(property));
                };
                paint.overflow_x = *value;
            }
            StyleProperty::OverflowY => {
                let StyleValue::Overflow(value) = value else {
                    return Err(invalid(property));
                };
                paint.overflow_y = *value;
            }
            StyleProperty::ZIndex => {
                let StyleValue::Integer(value) = value else {
                    return Err(invalid(property));
                };
                paint.z_index = i32::try_from(*value).map_err(|_| invalid(property))?;
            }
            _ => {}
        }
    }
    Ok(paint)
}

fn color(value: &StyleValue, property: StyleProperty) -> Result<ColorValue, StyleResolutionError> {
    let StyleValue::Color(value) = value else {
        return Err(invalid(property));
    };
    crate::resolution::normalize_color_for(value, property)
}

fn border_style(
    value: &StyleValue,
    property: StyleProperty,
) -> Result<BorderStyleValue, StyleResolutionError> {
    let StyleValue::BorderStyle(value) = value else {
        return Err(invalid(property));
    };
    Ok(*value)
}

fn radius(
    value: &StyleValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedCornerRadius, StyleResolutionError> {
    let (horizontal, vertical) = match value {
        StyleValue::LengthPercentage(value) => (value, value),
        StyleValue::BorderRadius(value) => (&value.horizontal, &value.vertical),
        _ => return Err(invalid(property)),
    };
    let horizontal = resolve_affine(horizontal, inherited.font_size(), environment, property)?;
    let vertical = resolve_affine(vertical, inherited.font_size(), environment, property)?;
    if horizontal.length() < 0.0
        || horizontal.fraction() < 0.0
        || vertical.length() < 0.0
        || vertical.fraction() < 0.0
    {
        return Err(invalid(property));
    }
    Ok(ComputedCornerRadius {
        horizontal,
        vertical,
    })
}

fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BorderRadiusValue, LengthPercentageValue, LengthUnit, LengthValue};

    fn number(value: f32) -> StyleNumber {
        StyleNumber::new(value)
    }

    fn px_length(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Length(LengthValue::Dimension {
            value: number(value),
            unit: LengthUnit::Px,
        })
    }

    fn px(value: f32) -> StyleValue {
        StyleValue::LengthPercentage(px_length(value))
    }

    #[test]
    fn paint_values_resolve_without_host_types() {
        let specified = SpecifiedStyle::new()
            .push(
                StyleProperty::Color,
                StyleValue::Color(ColorValue::Named("current".into())),
            )
            .push(
                StyleProperty::BackgroundColor,
                StyleValue::Color(ColorValue::Rgba {
                    red: 1,
                    green: 2,
                    blue: 3,
                    alpha: number(0.5),
                }),
            )
            .push(
                StyleProperty::BorderTopStyle,
                StyleValue::BorderStyle(BorderStyleValue::Solid),
            )
            .push(
                StyleProperty::BorderTopColor,
                StyleValue::Color(ColorValue::Named("top".into())),
            )
            .push(
                StyleProperty::BorderRightColor,
                StyleValue::Color(ColorValue::Named("right".into())),
            )
            .push(
                StyleProperty::BorderBottomColor,
                StyleValue::Color(ColorValue::Named("bottom".into())),
            )
            .push(
                StyleProperty::BorderLeftColor,
                StyleValue::Color(ColorValue::Named("left".into())),
            )
            .push(
                StyleProperty::BorderRightStyle,
                StyleValue::BorderStyle(BorderStyleValue::Dashed),
            )
            .push(
                StyleProperty::BorderBottomStyle,
                StyleValue::BorderStyle(BorderStyleValue::Dotted),
            )
            .push(
                StyleProperty::BorderLeftStyle,
                StyleValue::BorderStyle(BorderStyleValue::Double),
            )
            .push(StyleProperty::BorderTopLeftRadius, px(8.0))
            .push(
                StyleProperty::BorderTopRightRadius,
                StyleValue::BorderRadius(BorderRadiusValue {
                    horizontal: px_length(9.0),
                    vertical: px_length(4.0),
                }),
            )
            .push(StyleProperty::BorderBottomRightRadius, px(10.0))
            .push(StyleProperty::BorderBottomLeftRadius, px(11.0))
            .push(StyleProperty::Opacity, StyleValue::Number(number(2.0)))
            .push(
                StyleProperty::OverflowX,
                StyleValue::Overflow(OverflowValue::Hidden),
            )
            .push(
                StyleProperty::OverflowY,
                StyleValue::Overflow(OverflowValue::Hidden),
            )
            .push(
                StyleProperty::Visibility,
                StyleValue::Visibility(VisibilityValue::Hidden),
            )
            .push(StyleProperty::ZIndex, StyleValue::Integer(-3));
        let resolved = crate::resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
        let paint = resolved.computed().paint();
        assert_eq!(
            paint.background_color,
            ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: number(0.5),
            }
        );
        assert_eq!(paint.border_colors.top, ColorValue::Named("top".into()));
        assert_eq!(paint.border_colors.right, ColorValue::Named("right".into()));
        assert_eq!(
            paint.border_colors.bottom,
            ColorValue::Named("bottom".into())
        );
        assert_eq!(paint.border_colors.left, ColorValue::Named("left".into()));
        assert_eq!(paint.border_styles.top, BorderStyleValue::Solid);
        assert_eq!(paint.border_styles.right, BorderStyleValue::Dashed);
        assert_eq!(paint.border_styles.bottom, BorderStyleValue::Dotted);
        assert_eq!(paint.border_styles.left, BorderStyleValue::Double);
        assert_eq!(paint.border_radii.top_left.horizontal.length(), 8.0);
        assert_eq!(paint.border_radii.top_left.vertical.length(), 8.0);
        assert_eq!(paint.border_radii.top_right.horizontal.length(), 9.0);
        assert_eq!(paint.border_radii.top_right.vertical.length(), 4.0);
        assert_eq!(paint.border_radii.bottom_right.horizontal.length(), 10.0);
        assert_eq!(paint.border_radii.bottom_left.horizontal.length(), 11.0);
        assert_eq!(paint.opacity.get(), 1.0);
        assert_eq!(paint.overflow_x, OverflowValue::Hidden);
        assert_eq!(paint.overflow_y, OverflowValue::Hidden);
        assert_eq!(paint.visibility, VisibilityValue::Hidden);
        assert_eq!(paint.z_index, -3);

        assert!(paint.changes_from(paint).is_empty());
        let mut changed = paint.clone();
        changed.opacity = number(0.5);
        assert_eq!(changed.changes_from(paint), crate::PropertyImpactSet::PAINT);
    }

    #[test]
    fn invalid_paint_values_are_diagnostic() {
        for property in [
            StyleProperty::BackgroundColor,
            StyleProperty::BorderTopColor,
            StyleProperty::BorderRightColor,
            StyleProperty::BorderBottomColor,
            StyleProperty::BorderLeftColor,
            StyleProperty::BorderTopStyle,
            StyleProperty::BorderRightStyle,
            StyleProperty::BorderBottomStyle,
            StyleProperty::BorderLeftStyle,
            StyleProperty::BorderTopLeftRadius,
            StyleProperty::BorderTopRightRadius,
            StyleProperty::BorderBottomRightRadius,
            StyleProperty::BorderBottomLeftRadius,
            StyleProperty::Opacity,
            StyleProperty::Visibility,
            StyleProperty::OverflowX,
            StyleProperty::OverflowY,
            StyleProperty::ZIndex,
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::BorderTopLeftRadius, px(-1.0)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BorderTopLeftRadius
            ))
        );
        for (property, value) in [
            (
                StyleProperty::BackgroundColor,
                StyleValue::Color(ColorValue::Named(String::new())),
            ),
            (StyleProperty::Opacity, StyleValue::Number(number(f32::NAN))),
            (StyleProperty::ZIndex, StyleValue::Integer(i64::MAX)),
            (StyleProperty::BorderTopLeftRadius, px(f32::NAN)),
            (
                StyleProperty::BorderTopRightRadius,
                StyleValue::BorderRadius(BorderRadiusValue {
                    horizontal: px_length(1.0),
                    vertical: px_length(f32::NAN),
                }),
            ),
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(property, value),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(property))
            );
        }
    }
}
