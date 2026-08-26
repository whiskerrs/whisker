//! Lowering from computed style into the Host-independent paint protocol.

use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, OverflowClip, PaintColor, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, Transform, Visibility, VisualEffects,
};
use whisker_style::{
    BorderStyleValue, ColorValue, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedPaintStyle, ComputedTransformFunction, ComputedTransformStyle, OverflowValue,
    VisibilityValue,
};

/// Complete common presentation values derived from one computed node style.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredPaint {
    /// Background and border paint.
    pub box_paint: BoxPaint,
    /// Visual effects applied by the Host compositor.
    pub visual_effects: VisualEffects,
    /// Descendant overflow clip.
    pub clip: BoxClip,
    /// Transform retained until the border-box size is known.
    pub transform: ComputedTransformStyle,
    /// Group opacity.
    pub opacity: f32,
    /// Paint visibility.
    pub visibility: Visibility,
    /// Sibling stacking key.
    pub z_order: i32,
}

/// Lowers renderer-independent computed style into protocol-owned values.
pub fn lower_paint(style: &ComputedPaintStyle, layout: &ComputedLayoutStyle) -> LoweredPaint {
    LoweredPaint {
        box_paint: BoxPaint {
            background_color: lower_color(&style.background_color),
            border_widths: edges(&layout.border, length),
            border_colors: PaintEdges {
                top: effective_border_color(&style.border_colors.top, style.border_styles.top),
                right: effective_border_color(
                    &style.border_colors.right,
                    style.border_styles.right,
                ),
                bottom: effective_border_color(
                    &style.border_colors.bottom,
                    style.border_styles.bottom,
                ),
                left: effective_border_color(&style.border_colors.left, style.border_styles.left),
            },
            border_styles: edges(&style.border_styles, lower_border_style),
            border_radii: PaintCorners {
                top_left: corner_radius(&style.border_radii.top_left),
                top_right: corner_radius(&style.border_radii.top_right),
                bottom_right: corner_radius(&style.border_radii.bottom_right),
                bottom_left: corner_radius(&style.border_radii.bottom_left),
            },
        },
        visual_effects: VisualEffects {
            backdrop_blur: style.backdrop_blur.map(|value| value.get()),
            ..VisualEffects::default()
        },
        clip: BoxClip {
            horizontal: lower_overflow(style.overflow_x),
            vertical: lower_overflow(style.overflow_y),
        },
        transform: style.transform.clone(),
        opacity: style.opacity.get(),
        visibility: match style.visibility {
            VisibilityValue::Visible => Visibility::Visible,
            VisibilityValue::Hidden => Visibility::Hidden,
        },
        z_order: style.z_index,
    }
}

/// Resolves a computed transform against one node's border-box size.
///
/// Hosts receive only the resulting matrix; percentages and transform-origin
/// never cross the renderer boundary.
pub fn lower_transform(
    style: &ComputedTransformStyle,
    border_width: f32,
    border_height: f32,
) -> Option<Transform> {
    if !border_width.is_finite()
        || !border_height.is_finite()
        || border_width < 0.0
        || border_height < 0.0
    {
        return None;
    }

    let mut matrix = identity();
    for function in &style.functions {
        matrix = multiply(
            matrix,
            function_matrix(function, border_width, border_height)?,
        );
    }
    let origin_x = resolve_box_length(style.origin_x, border_width);
    let origin_y = resolve_box_length(style.origin_y, border_height);
    let around_origin = multiply(
        multiply(translation(origin_x, origin_y, 0.0), matrix),
        translation(-origin_x, -origin_y, 0.0),
    );
    around_origin
        .iter()
        .all(|value| value.is_finite())
        .then_some(Transform(around_origin))
}

fn function_matrix(
    function: &ComputedTransformFunction,
    border_width: f32,
    border_height: f32,
) -> Option<[f32; 16]> {
    let matrix = match function {
        ComputedTransformFunction::Translate { x, y, z } => translation(
            resolve_box_length(*x, border_width),
            resolve_box_length(*y, border_height),
            z.get(),
        ),
        ComputedTransformFunction::RotateX(degrees) => {
            let radians = degrees.get().to_radians();
            let (sin, cos) = radians.sin_cos();
            [
                1.0, 0.0, 0.0, 0.0, 0.0, cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        }
        ComputedTransformFunction::RotateY(degrees) => {
            let radians = degrees.get().to_radians();
            let (sin, cos) = radians.sin_cos();
            [
                cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        }
        ComputedTransformFunction::RotateZ(degrees) => {
            let radians = degrees.get().to_radians();
            let (sin, cos) = radians.sin_cos();
            [
                cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]
        }
        ComputedTransformFunction::Scale { x, y, z } => [
            x.get(),
            0.0,
            0.0,
            0.0,
            0.0,
            y.get(),
            0.0,
            0.0,
            0.0,
            0.0,
            z.get(),
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        ComputedTransformFunction::Skew {
            x_degrees,
            y_degrees,
        } => [
            1.0,
            y_degrees.get().to_radians().tan(),
            0.0,
            0.0,
            x_degrees.get().to_radians().tan(),
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
        ComputedTransformFunction::Matrix(values) => values.map(|value| value.get()),
    };
    matrix
        .iter()
        .all(|value| value.is_finite())
        .then_some(matrix)
}

fn identity() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
    ]
}

fn multiply(left: [f32; 16], right: [f32; 16]) -> [f32; 16] {
    let mut output = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            output[column * 4 + row] = (0..4)
                .map(|index| left[index * 4 + row] * right[column * 4 + index])
                .sum();
        }
    }
    output
}

fn resolve_box_length(value: ComputedLengthPercentage, extent: f32) -> f32 {
    value.length() + value.fraction() * extent
}

fn edges<T, U>(input: &whisker_style::Edges<T>, map: impl Fn(&T) -> U) -> PaintEdges<U> {
    PaintEdges {
        top: map(&input.top),
        right: map(&input.right),
        bottom: map(&input.bottom),
        left: map(&input.left),
    }
}

fn length(value: &ComputedLengthPercentage) -> PaintLengthPercentage {
    PaintLengthPercentage {
        length: value.length(),
        fraction: value.fraction(),
    }
}

fn corner_radius(value: &whisker_style::ComputedCornerRadius) -> PaintCornerRadius {
    PaintCornerRadius {
        horizontal: length(&value.horizontal),
        vertical: length(&value.vertical),
    }
}

/// Lowers a renderer-independent computed color into the paint protocol.
pub fn lower_color(color: &ColorValue) -> PaintColor {
    match color {
        ColorValue::Named(name) => PaintColor::Named(name.clone()),
        ColorValue::Rgba {
            red,
            green,
            blue,
            alpha,
        } => PaintColor::Srgba {
            red: *red,
            green: *green,
            blue: *blue,
            alpha: alpha.get(),
        },
        ColorValue::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => PaintColor::Hsla {
            hue_degrees: hue_degrees.get(),
            saturation: saturation.get(),
            lightness: lightness.get(),
            alpha: alpha.get(),
        },
    }
}

fn lower_border_style(value: &BorderStyleValue) -> BorderLineStyle {
    match value {
        BorderStyleValue::None => BorderLineStyle::None,
        BorderStyleValue::Hidden => BorderLineStyle::Hidden,
        BorderStyleValue::Solid => BorderLineStyle::Solid,
        BorderStyleValue::Dashed => BorderLineStyle::Dashed,
        BorderStyleValue::Dotted => BorderLineStyle::Dotted,
        BorderStyleValue::Double => BorderLineStyle::Double,
        BorderStyleValue::Groove => BorderLineStyle::Groove,
        BorderStyleValue::Ridge => BorderLineStyle::Ridge,
        BorderStyleValue::Inset => BorderLineStyle::Inset,
        BorderStyleValue::Outset => BorderLineStyle::Outset,
    }
}

fn effective_border_color(color: &ColorValue, style: BorderStyleValue) -> PaintColor {
    if matches!(style, BorderStyleValue::None | BorderStyleValue::Hidden) {
        PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        }
    } else {
        lower_color(color)
    }
}

fn lower_overflow(value: OverflowValue) -> OverflowClip {
    match value {
        OverflowValue::Visible => OverflowClip::Visible,
        OverflowValue::Hidden => OverflowClip::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_style::{ComputedCornerRadius, Corners, Edges, StyleNumber};

    fn color(name: &str) -> ColorValue {
        ColorValue::Named(name.into())
    }

    fn paint_style() -> ComputedPaintStyle {
        ComputedPaintStyle {
            background_color: color("background"),
            background_images: Vec::new(),
            background_layers: vec![Default::default()],
            border_colors: Edges {
                top: color("top"),
                right: color("right"),
                bottom: color("bottom"),
                left: color("left"),
            },
            border_styles: Edges {
                top: BorderStyleValue::Solid,
                right: BorderStyleValue::Dashed,
                bottom: BorderStyleValue::Dotted,
                left: BorderStyleValue::Double,
            },
            border_radii: Corners {
                top_left: radius(1.0, 0.1, 11.0, 0.11),
                top_right: radius(2.0, 0.2, 12.0, 0.12),
                bottom_right: radius(3.0, 0.3, 13.0, 0.13),
                bottom_left: radius(4.0, 0.4, 14.0, 0.14),
            },
            transform: ComputedTransformStyle::default(),
            opacity: StyleNumber::new(0.5),
            visibility: VisibilityValue::Hidden,
            overflow_x: OverflowValue::Visible,
            overflow_y: OverflowValue::Hidden,
            z_index: -3,
            backdrop_blur: Some(StyleNumber::new(8.0)),
        }
    }

    fn radius(
        horizontal_length: f32,
        horizontal_fraction: f32,
        vertical_length: f32,
        vertical_fraction: f32,
    ) -> ComputedCornerRadius {
        ComputedCornerRadius {
            horizontal: ComputedLengthPercentage::new(horizontal_length, horizontal_fraction),
            vertical: ComputedLengthPercentage::new(vertical_length, vertical_fraction),
        }
    }

    #[test]
    fn lowers_complete_box_paint_clip_and_compositing_state() {
        let layout = ComputedLayoutStyle {
            border: Edges {
                top: ComputedLengthPercentage::new(1.0, 0.0),
                right: ComputedLengthPercentage::new(2.0, 0.1),
                bottom: ComputedLengthPercentage::new(3.0, 0.2),
                left: ComputedLengthPercentage::new(4.0, 0.3),
            },
            ..ComputedLayoutStyle::default()
        };
        let lowered = lower_paint(&paint_style(), &layout);

        assert_eq!(
            lowered.box_paint.background_color,
            PaintColor::Named("background".into())
        );
        assert_eq!(lowered.box_paint.border_widths.left.length, 4.0);
        assert_eq!(lowered.box_paint.border_widths.left.fraction, 0.3);
        assert_eq!(
            lowered.box_paint.border_radii.bottom_left.horizontal.length,
            4.0
        );
        assert_eq!(
            lowered.box_paint.border_radii.bottom_left.vertical.length,
            14.0
        );
        assert_eq!(
            lowered.box_paint.border_colors.top,
            PaintColor::Named("top".into())
        );
        assert_eq!(
            lowered.box_paint.border_styles.right,
            BorderLineStyle::Dashed
        );
        assert_eq!(lowered.clip.horizontal, OverflowClip::Visible);
        assert_eq!(lowered.clip.vertical, OverflowClip::Hidden);
        assert_eq!(lowered.opacity, 0.5);
        assert_eq!(lowered.visibility, Visibility::Hidden);
        assert_eq!(lowered.z_order, -3);
        assert_eq!(lowered.visual_effects.backdrop_blur, Some(8.0));
    }

    #[test]
    fn resolves_transform_percentages_and_origin_against_border_box() {
        let style = ComputedTransformStyle {
            functions: vec![ComputedTransformFunction::Scale {
                x: StyleNumber::new(2.0),
                y: StyleNumber::new(2.0),
                z: StyleNumber::new(1.0),
            }],
            origin_x: ComputedLengthPercentage::new(0.0, 0.25),
            origin_y: ComputedLengthPercentage::new(0.0, 0.5),
        };
        assert_eq!(
            lower_transform(&style, 40.0, 20.0),
            Some(Transform([
                2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -10.0, -10.0, 0.0, 1.0,
            ]))
        );

        let translated = ComputedTransformStyle {
            functions: vec![ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::new(3.0, 0.5),
                y: ComputedLengthPercentage::new(4.0, 0.25),
                z: StyleNumber::new(0.0),
            }],
            origin_x: ComputedLengthPercentage::ZERO,
            origin_y: ComputedLengthPercentage::ZERO,
        };
        let matrix = lower_transform(&translated, 40.0, 20.0).unwrap();
        assert_eq!(matrix.0[12], 23.0);
        assert_eq!(matrix.0[13], 9.0);
        assert_eq!(lower_transform(&translated, f32::NAN, 20.0), None);
    }

    #[test]
    fn lowers_every_flat_transform_function_and_rejects_invalid_output() {
        let number = StyleNumber::new;
        for function in [
            ComputedTransformFunction::RotateX(number(0.0)),
            ComputedTransformFunction::RotateY(number(0.0)),
            ComputedTransformFunction::RotateZ(number(90.0)),
            ComputedTransformFunction::Skew {
                x_degrees: number(10.0),
                y_degrees: number(20.0),
            },
            ComputedTransformFunction::Matrix([
                number(1.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(1.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(1.0),
                number(0.0),
                number(2.0),
                number(3.0),
                number(0.0),
                number(1.0),
            ]),
        ] {
            let style = ComputedTransformStyle {
                functions: vec![function],
                origin_x: ComputedLengthPercentage::ZERO,
                origin_y: ComputedLengthPercentage::ZERO,
            };
            assert!(lower_transform(&style, 40.0, 20.0).is_some());
        }

        let mut non_finite = [number(0.0); 16];
        non_finite[0] = number(f32::NAN);
        let invalid_function = ComputedTransformStyle {
            functions: vec![ComputedTransformFunction::Matrix(non_finite)],
            ..ComputedTransformStyle::default()
        };
        assert_eq!(lower_transform(&invalid_function, 1.0, 1.0), None);

        let invalid_origin = ComputedTransformStyle {
            origin_x: ComputedLengthPercentage::new(f32::INFINITY, 0.0),
            ..ComputedTransformStyle::default()
        };
        assert_eq!(lower_transform(&invalid_origin, 1.0, 1.0), None);
    }

    #[test]
    fn lowers_every_color_border_visibility_and_overflow_variant() {
        assert_eq!(
            lower_color(&ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: StyleNumber::new(0.4),
            }),
            PaintColor::Srgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: 0.4,
            }
        );
        assert_eq!(
            lower_color(&ColorValue::Hsla {
                hue_degrees: StyleNumber::new(10.0),
                saturation: StyleNumber::new(20.0),
                lightness: StyleNumber::new(30.0),
                alpha: StyleNumber::new(0.5),
            }),
            PaintColor::Hsla {
                hue_degrees: 10.0,
                saturation: 20.0,
                lightness: 30.0,
                alpha: 0.5,
            }
        );

        for (source, expected) in [
            (BorderStyleValue::None, BorderLineStyle::None),
            (BorderStyleValue::Hidden, BorderLineStyle::Hidden),
            (BorderStyleValue::Solid, BorderLineStyle::Solid),
            (BorderStyleValue::Dashed, BorderLineStyle::Dashed),
            (BorderStyleValue::Dotted, BorderLineStyle::Dotted),
            (BorderStyleValue::Double, BorderLineStyle::Double),
            (BorderStyleValue::Groove, BorderLineStyle::Groove),
            (BorderStyleValue::Ridge, BorderLineStyle::Ridge),
            (BorderStyleValue::Inset, BorderLineStyle::Inset),
            (BorderStyleValue::Outset, BorderLineStyle::Outset),
        ] {
            assert_eq!(lower_border_style(&source), expected);
        }

        let transparent = PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        };
        assert_eq!(
            effective_border_color(&color("ignored"), BorderStyleValue::None),
            transparent
        );
        assert_eq!(
            effective_border_color(&color("ignored"), BorderStyleValue::Hidden),
            transparent
        );
        assert_eq!(
            effective_border_color(&color("kept"), BorderStyleValue::Solid),
            PaintColor::Named("kept".into())
        );
        assert_eq!(
            lower_overflow(OverflowValue::Visible),
            OverflowClip::Visible
        );
        assert_eq!(lower_overflow(OverflowValue::Hidden), OverflowClip::Hidden);

        let mut visible = paint_style();
        visible.visibility = VisibilityValue::Visible;
        assert_eq!(
            lower_paint(&visible, &ComputedLayoutStyle::default()).visibility,
            Visibility::Visible
        );
    }
}
