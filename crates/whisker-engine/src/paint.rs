//! Lowering from computed style into the Host-independent paint protocol.

use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, OverflowClip, PaintColor, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, Transform, Visibility, VisualEffects,
};
use whisker_style::{
    BorderStyleValue, ColorValue, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedPaintStyle, ComputedTransformFunction, ComputedTransformStyle, MotionPathCommandValue,
    OffsetPathValue, OffsetRotateValue, OverflowValue, VisibilityValue,
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

    // Lynx intentionally differs from browser CSS here: `perspective` affects
    // the current node. Prepending it makes the node's transform functions run
    // first and the perspective divide run afterward.
    let mut matrix = style
        .perspective
        .map(|distance| perspective(distance.get().max(1.0)))
        .unwrap_or_else(identity);
    let motion = match &style.offset_path {
        OffsetPathValue::None => None,
        path => Some(motion_path_state(path, style.offset_distance.get())?),
    };
    if let Some((_, _, tangent)) = motion {
        let angle = match style.offset_rotate {
            OffsetRotateValue::Auto => tangent,
            OffsetRotateValue::Angle(angle) => angle.get(),
        };
        matrix = multiply(matrix, rotation_z(angle));
    }
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
    // Every Host currently flattens at the node boundary. Preserve the exact
    // X/Y/W projection of points on the local z=0 plane, but canonicalize the
    // unused depth row and column so GPU clip-space depth cannot discard CSS
    // pixels and descendants cannot accidentally share a 3-D context.
    let positioned = motion.map_or(around_origin, |(x, y, _)| {
        multiply(translation(x, y, 0.0), around_origin)
    });
    let flat_plane = [
        positioned[0],
        positioned[1],
        0.0,
        positioned[3],
        positioned[4],
        positioned[5],
        0.0,
        positioned[7],
        0.0,
        0.0,
        1.0,
        0.0,
        positioned[12],
        positioned[13],
        0.0,
        positioned[15],
    ];
    flat_plane
        .iter()
        .all(|value| value.is_finite())
        .then_some(Transform(flat_plane))
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
        ComputedTransformFunction::RotateZ(degrees) => rotation_z(degrees.get()),
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

fn motion_path_state(path: &OffsetPathValue, progress: f32) -> Option<(f32, f32, f32)> {
    let OffsetPathValue::Path(commands) = path else {
        return None;
    };
    let mut segments = Vec::new();
    let mut current = None;
    let mut subpath_start = None;
    let mut total_length = 0.0_f32;
    for command in commands {
        let next = match *command {
            MotionPathCommandValue::MoveTo(point) => {
                let point = (point.x.get(), point.y.get());
                current = Some(point);
                subpath_start = Some(point);
                continue;
            }
            MotionPathCommandValue::LineTo(point) => (point.x.get(), point.y.get()),
            MotionPathCommandValue::Close => subpath_start?,
        };
        let from = current?;
        let length = (next.0 - from.0).hypot(next.1 - from.1);
        if length > 0.0 && length.is_finite() {
            segments.push((from, next, length));
            total_length += length;
        }
        current = Some(next);
    }
    if segments.is_empty() || !total_length.is_finite() {
        return None;
    }
    let target = progress.clamp(0.0, 1.0) * total_length;
    let mut traversed = 0.0;
    let (last, preceding) = segments
        .split_last()
        .expect("motion path segments were checked as non-empty");
    for (from, to, length) in preceding {
        if target <= traversed + length {
            let local = ((target - traversed) / length).clamp(0.0, 1.0);
            let x = from.0 + (to.0 - from.0) * local;
            let y = from.1 + (to.1 - from.1) * local;
            let angle = (to.1 - from.1).atan2(to.0 - from.0).to_degrees();
            return Some((x, y, angle.rem_euclid(360.0)));
        }
        traversed += length;
    }
    let (from, to, length) = *last;
    let local = ((target - traversed) / length).clamp(0.0, 1.0);
    let x = from.0 + (to.0 - from.0) * local;
    let y = from.1 + (to.1 - from.1) * local;
    let angle = (to.1 - from.1).atan2(to.0 - from.0).to_degrees();
    Some((x, y, angle.rem_euclid(360.0)))
}

fn rotation_z(degrees: f32) -> [f32; 16] {
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    [
        cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
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

fn perspective(distance: f32) -> [f32; 16] {
    let mut matrix = identity();
    matrix[11] = -1.0 / distance;
    matrix
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
    use whisker_style::{ComputedCornerRadius, Corners, Edges, MotionPathPointValue, StyleNumber};

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
            perspective: None,
            offset_path: OffsetPathValue::None,
            offset_distance: StyleNumber::new(0.0),
            offset_rotate: OffsetRotateValue::Auto,
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
            perspective: None,
            offset_path: OffsetPathValue::None,
            offset_distance: StyleNumber::new(0.0),
            offset_rotate: OffsetRotateValue::Auto,
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
                perspective: None,
                offset_path: OffsetPathValue::None,
                offset_distance: StyleNumber::new(0.0),
                offset_rotate: OffsetRotateValue::Auto,
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
    fn canonicalizes_three_dimensional_output_to_the_node_plane() {
        let style = ComputedTransformStyle {
            perspective: None,
            offset_path: OffsetPathValue::None,
            offset_distance: StyleNumber::new(0.0),
            offset_rotate: OffsetRotateValue::Auto,
            functions: vec![ComputedTransformFunction::RotateY(StyleNumber::new(60.0))],
            origin_x: ComputedLengthPercentage::ZERO,
            origin_y: ComputedLengthPercentage::ZERO,
        };
        let transform = lower_transform(&style, 40.0, 20.0).expect("finite transform");
        let expected = [
            0.5, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        for (actual, expected) in transform.0.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.000_001,
                "{actual} != {expected}"
            );
        }
    }

    #[test]
    fn prepends_lynx_current_node_perspective_to_the_transform_matrix() {
        let style = ComputedTransformStyle {
            perspective: Some(StyleNumber::new(100.0)),
            offset_path: OffsetPathValue::None,
            offset_distance: StyleNumber::new(0.0),
            offset_rotate: OffsetRotateValue::Auto,
            functions: vec![ComputedTransformFunction::RotateY(StyleNumber::new(60.0))],
            origin_x: ComputedLengthPercentage::ZERO,
            origin_y: ComputedLengthPercentage::ZERO,
        };
        let transform = lower_transform(&style, 40.0, 20.0).expect("finite perspective");
        let expected = [
            0.5,
            0.0,
            0.0,
            3.0_f32.sqrt() / 200.0,
            0.0,
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
        ];
        for (actual, expected) in transform.0.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.000_001,
                "{actual} != {expected}"
            );
        }
    }

    #[test]
    fn lowers_polyline_motion_progress_auto_rotation_and_post_translation() {
        let point = |x, y| MotionPathPointValue {
            x: StyleNumber::new(x),
            y: StyleNumber::new(y),
        };
        assert_eq!(motion_path_state(&OffsetPathValue::None, 0.5), None);
        let style = ComputedTransformStyle {
            offset_path: OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::LineTo(point(40.0, 0.0)),
                MotionPathCommandValue::LineTo(point(40.0, 30.0)),
            ]),
            offset_distance: StyleNumber::new(0.75),
            offset_rotate: OffsetRotateValue::Auto,
            ..ComputedTransformStyle::default()
        };
        let transform = lower_transform(&style, 20.0, 10.0).unwrap();
        let expected = [
            0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 55.0, 7.5, 0.0, 1.0,
        ];
        for (actual, expected) in transform.0.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.000_01,
                "{actual} != {expected}"
            );
        }

        let first_segment = ComputedTransformStyle {
            offset_distance: StyleNumber::new(0.25),
            ..style.clone()
        };
        let transform = lower_transform(&first_segment, 20.0, 10.0).unwrap();
        assert_eq!(transform.0[12], 17.5);
        assert_eq!(transform.0[13], 0.0);

        let fixed = ComputedTransformStyle {
            offset_path: OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::LineTo(point(10.0, 0.0)),
                MotionPathCommandValue::Close,
            ]),
            offset_distance: StyleNumber::new(1.0),
            offset_rotate: OffsetRotateValue::Angle(StyleNumber::new(0.0)),
            ..ComputedTransformStyle::default()
        };
        let transform = lower_transform(&fixed, 1.0, 1.0).unwrap();
        assert_eq!(transform.0[12], 0.0);
        assert_eq!(transform.0[13], 0.0);

        for offset_path in [
            OffsetPathValue::Path(Vec::new()),
            OffsetPathValue::Path(vec![MotionPathCommandValue::LineTo(point(1.0, 0.0))]),
            OffsetPathValue::Path(vec![MotionPathCommandValue::Close]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(1.0, 1.0)),
                MotionPathCommandValue::LineTo(point(1.0, 1.0)),
            ]),
        ] {
            assert_eq!(
                lower_transform(
                    &ComputedTransformStyle {
                        offset_path,
                        ..ComputedTransformStyle::default()
                    },
                    1.0,
                    1.0,
                ),
                None
            );
        }
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
