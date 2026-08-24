//! Computed paint values that remain independent of every Host renderer.

use crate::{
    BackgroundAttachmentValue, BackgroundBoxValue, BackgroundImageValue, BackgroundPositionValue,
    BackgroundRepeatModeValue, BackgroundSizeValue, ColorValue, ComputedLengthPercentage, Edges,
    InheritedStyle, SpecifiedStyle, StyleEnvironment, StyleNumber, StyleProperty,
    StyleResolutionError, StyleValue, layout::resolve_affine,
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

/// Computed position of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedBackgroundPosition {
    /// Horizontal length plus positioning-area fraction.
    pub horizontal: ComputedLengthPercentage,
    /// Vertical length plus positioning-area fraction.
    pub vertical: ComputedLengthPercentage,
}

/// Computed size of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedBackgroundSize {
    /// Use the image's intrinsic dimensions.
    Auto,
    /// Preserve aspect ratio while covering the positioning area.
    Cover,
    /// Preserve aspect ratio while fitting inside the positioning area.
    Contain,
    /// Resolve an explicit width and height against the positioning area.
    Explicit {
        /// Computed image width, or intrinsic width for `auto`.
        width: Option<ComputedLengthPercentage>,
        /// Computed image height, or intrinsic height for `auto`.
        height: Option<ComputedLengthPercentage>,
    },
}

/// Computed geometry and scrolling values paired with one background image.
///
/// The authoring API currently exposes one scalar set of longhands. Keeping
/// them grouped makes later CSS list alignment an expansion from one layer to
/// many rather than a collection of unrelated parallel fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedBackgroundLayerStyle {
    /// Position within the selected origin box.
    pub position: ComputedBackgroundPosition,
    /// Image sizing behavior.
    pub size: ComputedBackgroundSize,
    /// Horizontal tiling behavior.
    pub repeat_x: BackgroundRepeatModeValue,
    /// Vertical tiling behavior.
    pub repeat_y: BackgroundRepeatModeValue,
    /// Box defining the positioning area.
    pub origin: BackgroundBoxValue,
    /// Box clipping background paint.
    pub clip: BackgroundBoxValue,
    /// Relationship to scrolling.
    pub attachment: BackgroundAttachmentValue,
}

impl Default for ComputedBackgroundLayerStyle {
    fn default() -> Self {
        Self {
            position: ComputedBackgroundPosition {
                horizontal: ComputedLengthPercentage::ZERO,
                vertical: ComputedLengthPercentage::ZERO,
            },
            size: ComputedBackgroundSize::Auto,
            repeat_x: BackgroundRepeatModeValue::Repeat,
            repeat_y: BackgroundRepeatModeValue::Repeat,
            origin: BackgroundBoxValue::Padding,
            clip: BackgroundBoxValue::Border,
            attachment: BackgroundAttachmentValue::Scroll,
        }
    }
}

/// Computed background, border, clip, and compositing values for one node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedPaintStyle {
    /// Resolved background color. Transparent is represented explicitly.
    pub background_color: ColorValue,
    /// Ordered Host-independent background image sources, front to back.
    pub background_images: Vec<BackgroundImageValue>,
    /// Scalar longhands applied to the current background image layer.
    pub background_layer: ComputedBackgroundLayerStyle,
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
            background_images: Vec::new(),
            background_layer: ComputedBackgroundLayerStyle::default(),
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
            StyleProperty::BackgroundImage => {
                let StyleValue::BackgroundImages(images) = value else {
                    return Err(invalid(property));
                };
                paint.background_images = images.clone();
            }
            StyleProperty::BackgroundRepeat => {
                let StyleValue::BackgroundRepeat(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.repeat_x = value.horizontal;
                paint.background_layer.repeat_y = value.vertical;
            }
            StyleProperty::BackgroundPosition => {
                let StyleValue::BackgroundPosition(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.position =
                    resolve_background_position(value, inherited, environment, property)?;
            }
            StyleProperty::BackgroundPositionX => {
                paint.background_layer.position.horizontal =
                    resolve_length_percentage(value, inherited, environment, property)?;
            }
            StyleProperty::BackgroundPositionY => {
                paint.background_layer.position.vertical =
                    resolve_length_percentage(value, inherited, environment, property)?;
            }
            StyleProperty::BackgroundSize => {
                let StyleValue::BackgroundSize(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.size =
                    resolve_background_size(value, inherited, environment, property)?;
            }
            StyleProperty::BackgroundOrigin => {
                let StyleValue::BackgroundBox(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.origin = *value;
            }
            StyleProperty::BackgroundClip => {
                let StyleValue::BackgroundBox(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.clip = *value;
            }
            StyleProperty::BackgroundAttachment => {
                let StyleValue::BackgroundAttachment(value) = value else {
                    return Err(invalid(property));
                };
                paint.background_layer.attachment = *value;
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

fn resolve_length_percentage(
    value: &StyleValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentage, StyleResolutionError> {
    let StyleValue::LengthPercentage(value) = value else {
        return Err(invalid(property));
    };
    resolve_affine(value, inherited.font_size(), environment, property)
}

fn resolve_background_position(
    value: &BackgroundPositionValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBackgroundPosition, StyleResolutionError> {
    Ok(ComputedBackgroundPosition {
        horizontal: resolve_affine(
            &value.horizontal,
            inherited.font_size(),
            environment,
            property,
        )?,
        vertical: resolve_affine(
            &value.vertical,
            inherited.font_size(),
            environment,
            property,
        )?,
    })
}

fn resolve_background_size(
    value: &BackgroundSizeValue,
    inherited: &InheritedStyle,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedBackgroundSize, StyleResolutionError> {
    let size = match value {
        BackgroundSizeValue::Auto => ComputedBackgroundSize::Auto,
        BackgroundSizeValue::Cover => ComputedBackgroundSize::Cover,
        BackgroundSizeValue::Contain => ComputedBackgroundSize::Contain,
        BackgroundSizeValue::Explicit { width, height } => {
            let resolve_axis = |value: &Option<_>| {
                value
                    .as_ref()
                    .map(|value| {
                        resolve_affine(value, inherited.font_size(), environment, property)
                    })
                    .transpose()
            };
            let width = resolve_axis(width)?;
            let height = resolve_axis(height)?;
            if width
                .into_iter()
                .chain(height)
                .any(|value| value.length() < 0.0 || value.fraction() < 0.0)
            {
                return Err(invalid(property));
            }
            if width.is_none() && height.is_none() {
                ComputedBackgroundSize::Auto
            } else {
                ComputedBackgroundSize::Explicit { width, height }
            }
        }
    };
    Ok(size)
}

fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BackgroundRepeatValue, BorderRadiusValue, LengthPercentageValue, LengthUnit, LengthValue,
    };

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

    fn percentage(value: f32) -> LengthPercentageValue {
        LengthPercentageValue::Percentage(number(value))
    }

    #[test]
    fn logical_borders_resolve_to_physical_edges_and_corners() {
        let specified = |direction| {
            SpecifiedStyle::new()
                .push(StyleProperty::Direction, StyleValue::Direction(direction))
                .push(StyleProperty::BorderInlineStartWidth, px(2.0))
                .push(StyleProperty::BorderInlineEndWidth, px(3.0))
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("start".into())),
                )
                .push(
                    StyleProperty::BorderInlineEndColor,
                    StyleValue::Color(ColorValue::Named("end".into())),
                )
                .push(
                    StyleProperty::BorderInlineStartStyle,
                    StyleValue::BorderStyle(BorderStyleValue::Dotted),
                )
                .push(
                    StyleProperty::BorderInlineEndStyle,
                    StyleValue::BorderStyle(BorderStyleValue::Double),
                )
                .push(StyleProperty::BorderStartStartRadius, px(11.0))
                .push(StyleProperty::BorderStartEndRadius, px(12.0))
                .push(StyleProperty::BorderEndStartRadius, px(13.0))
                .push(StyleProperty::BorderEndEndRadius, px(14.0))
        };

        for (direction, start_is_left) in [
            (crate::DirectionValue::Ltr, true),
            (crate::DirectionValue::Rtl, false),
        ] {
            let resolved =
                crate::resolve_style(&specified(direction), None, StyleEnvironment::default())
                    .unwrap();
            let layout = resolved.computed().layout();
            let paint = resolved.computed().paint();
            let (start_width, end_width) = if start_is_left {
                (layout.border.left.length(), layout.border.right.length())
            } else {
                (layout.border.right.length(), layout.border.left.length())
            };
            assert_eq!((start_width, end_width), (2.0, 3.0));

            let (start_color, end_color, start_style, end_style) = if start_is_left {
                (
                    &paint.border_colors.left,
                    &paint.border_colors.right,
                    paint.border_styles.left,
                    paint.border_styles.right,
                )
            } else {
                (
                    &paint.border_colors.right,
                    &paint.border_colors.left,
                    paint.border_styles.right,
                    paint.border_styles.left,
                )
            };
            assert_eq!(start_color, &ColorValue::Named("start".into()));
            assert_eq!(end_color, &ColorValue::Named("end".into()));
            assert_eq!(start_style, BorderStyleValue::Dotted);
            assert_eq!(end_style, BorderStyleValue::Double);

            let corners = &paint.border_radii;
            let logical = if start_is_left {
                [
                    corners.top_left,
                    corners.top_right,
                    corners.bottom_left,
                    corners.bottom_right,
                ]
            } else {
                [
                    corners.top_right,
                    corners.top_left,
                    corners.bottom_right,
                    corners.bottom_left,
                ]
            };
            assert_eq!(
                logical.map(|corner| corner.horizontal.length()),
                [11.0, 12.0, 13.0, 14.0]
            );
        }
    }

    #[test]
    fn logical_and_physical_border_declarations_share_final_write_order() {
        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(StyleProperty::BorderInlineStartWidth, px(2.0))
                .push(StyleProperty::BorderLeftWidth, px(4.0))
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("logical".into())),
                )
                .push(
                    StyleProperty::BorderLeftColor,
                    StyleValue::Color(ColorValue::Named("physical".into())),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().layout().border.left.length(), 4.0);
        assert_eq!(
            resolved.computed().paint().border_colors.left,
            ColorValue::Named("physical".into())
        );

        let resolved = crate::resolve_style(
            &SpecifiedStyle::new()
                .push(StyleProperty::BorderLeftWidth, px(4.0))
                .push(StyleProperty::BorderInlineStartWidth, px(6.0))
                .push(
                    StyleProperty::BorderLeftColor,
                    StyleValue::Color(ColorValue::Named("physical".into())),
                )
                .push(
                    StyleProperty::BorderInlineStartColor,
                    StyleValue::Color(ColorValue::Named("logical".into())),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(resolved.computed().layout().border.left.length(), 6.0);
        assert_eq!(
            resolved.computed().paint().border_colors.left,
            ColorValue::Named("logical".into())
        );
    }

    #[test]
    fn background_layer_initial_values_match_css() {
        let resolved =
            crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
                .unwrap();
        assert_eq!(
            resolved.computed().paint().background_layer,
            ComputedBackgroundLayerStyle::default()
        );
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
                StyleProperty::BackgroundImage,
                StyleValue::BackgroundImages(vec![BackgroundImageValue::Url(
                    "https://example.com/image.png".into(),
                )]),
            )
            .push(
                StyleProperty::BackgroundPosition,
                StyleValue::BackgroundPosition(BackgroundPositionValue {
                    horizontal: percentage(25.0),
                    vertical: px_length(10.0),
                }),
            )
            .push(StyleProperty::BackgroundPositionX, px(5.0))
            .push(
                StyleProperty::BackgroundSize,
                StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                    width: Some(percentage(50.0)),
                    height: Some(px_length(20.0)),
                }),
            )
            .push(
                StyleProperty::BackgroundRepeat,
                StyleValue::BackgroundRepeat(BackgroundRepeatValue {
                    horizontal: BackgroundRepeatModeValue::Space,
                    vertical: BackgroundRepeatModeValue::Round,
                }),
            )
            .push(
                StyleProperty::BackgroundOrigin,
                StyleValue::BackgroundBox(BackgroundBoxValue::Content),
            )
            .push(
                StyleProperty::BackgroundClip,
                StyleValue::BackgroundBox(BackgroundBoxValue::Padding),
            )
            .push(
                StyleProperty::BackgroundAttachment,
                StyleValue::BackgroundAttachment(BackgroundAttachmentValue::Scroll),
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
        assert_eq!(
            paint.background_images,
            vec![BackgroundImageValue::Url(
                "https://example.com/image.png".into()
            )]
        );
        assert_eq!(paint.background_layer.position.horizontal.length(), 5.0);
        assert_eq!(paint.background_layer.position.horizontal.fraction(), 0.0);
        assert_eq!(paint.background_layer.position.vertical.length(), 10.0);
        assert_eq!(
            paint.background_layer.size,
            ComputedBackgroundSize::Explicit {
                width: Some(ComputedLengthPercentage::new(0.0, 0.5)),
                height: Some(ComputedLengthPercentage::new(20.0, 0.0)),
            }
        );
        assert_eq!(
            paint.background_layer.repeat_x,
            BackgroundRepeatModeValue::Space
        );
        assert_eq!(
            paint.background_layer.repeat_y,
            BackgroundRepeatModeValue::Round
        );
        assert_eq!(paint.background_layer.origin, BackgroundBoxValue::Content);
        assert_eq!(paint.background_layer.clip, BackgroundBoxValue::Padding);
        assert_eq!(
            paint.background_layer.attachment,
            BackgroundAttachmentValue::Scroll
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
    fn background_geometry_resolves_keywords_auto_axes_and_axis_errors() {
        let resolve_size = |size| {
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::BackgroundSize,
                    StyleValue::BackgroundSize(size),
                ),
                None,
                StyleEnvironment::default(),
            )
        };
        for (specified, computed) in [
            (BackgroundSizeValue::Auto, ComputedBackgroundSize::Auto),
            (BackgroundSizeValue::Cover, ComputedBackgroundSize::Cover),
            (
                BackgroundSizeValue::Contain,
                ComputedBackgroundSize::Contain,
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: None,
                    height: None,
                },
                ComputedBackgroundSize::Auto,
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: Some(px_length(12.0)),
                    height: None,
                },
                ComputedBackgroundSize::Explicit {
                    width: Some(ComputedLengthPercentage::new(12.0, 0.0)),
                    height: None,
                },
            ),
            (
                BackgroundSizeValue::Explicit {
                    width: None,
                    height: Some(percentage(25.0)),
                },
                ComputedBackgroundSize::Explicit {
                    width: None,
                    height: Some(ComputedLengthPercentage::new(0.0, 0.25)),
                },
            ),
        ] {
            assert_eq!(
                resolve_size(specified)
                    .unwrap()
                    .computed()
                    .paint()
                    .background_layer
                    .size,
                computed
            );
        }

        for position in [
            BackgroundPositionValue {
                horizontal: px_length(f32::NAN),
                vertical: px_length(0.0),
            },
            BackgroundPositionValue {
                horizontal: px_length(0.0),
                vertical: px_length(f32::NAN),
            },
        ] {
            assert_eq!(
                crate::resolve_style(
                    &SpecifiedStyle::new().push(
                        StyleProperty::BackgroundPosition,
                        StyleValue::BackgroundPosition(position),
                    ),
                    None,
                    StyleEnvironment::default(),
                ),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackgroundPosition
                ))
            );
        }

        for size in [
            BackgroundSizeValue::Explicit {
                width: Some(px_length(f32::NAN)),
                height: None,
            },
            BackgroundSizeValue::Explicit {
                width: None,
                height: Some(px_length(f32::NAN)),
            },
            BackgroundSizeValue::Explicit {
                width: None,
                height: Some(px_length(-1.0)),
            },
        ] {
            assert_eq!(
                resolve_size(size),
                Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::BackgroundSize
                ))
            );
        }
    }

    #[test]
    fn invalid_paint_values_are_diagnostic() {
        for property in [
            StyleProperty::BackgroundColor,
            StyleProperty::BackgroundImage,
            StyleProperty::BackgroundRepeat,
            StyleProperty::BackgroundPosition,
            StyleProperty::BackgroundPositionX,
            StyleProperty::BackgroundPositionY,
            StyleProperty::BackgroundSize,
            StyleProperty::BackgroundOrigin,
            StyleProperty::BackgroundClip,
            StyleProperty::BackgroundAttachment,
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
        let inherited =
            crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
                .unwrap()
                .computed()
                .inherited_text()
                .clone();
        for property in [StyleProperty::OverflowX, StyleProperty::OverflowY] {
            assert_eq!(
                resolve_paint_style(
                    &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                    &inherited,
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
            (
                StyleProperty::BackgroundSize,
                StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                    width: Some(px_length(-1.0)),
                    height: Some(px_length(1.0)),
                }),
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
