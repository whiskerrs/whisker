//! Lowering from computed style into the Host-independent paint protocol.

use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, BoxShadow, ClipShape, FillRule, ImageRendering,
    OverflowClip, PaintBox, PaintColor, PaintCoordinate, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, PaintPosition, PathCommand, Transform, Visibility,
    VisualEffects,
};
use whisker_style::{
    BorderStyleValue, ClipBoxValue, ClipFillRuleValue, ColorValue, ComputedClipPath,
    ComputedClipPathCommand, ComputedClipPoint, ComputedClipShape, ComputedLayoutStyle,
    ComputedLengthPercentage, ComputedOffsetPathValue as OffsetPathValue, ComputedPaintStyle,
    ComputedTransformFunction, ComputedTransformStyle, ImageRenderingValue, MotionPathCommandValue,
    OffsetRotateValue, OverflowValue, VisibilityValue,
};

use crate::color::named_color_srgb;

mod motion_path;

use motion_path::motion_path_state;
#[cfg(test)]
use motion_path::{MotionCurve, MotionSegment, append_rotated_ellipse, point_line_distance};

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
            box_shadows: style
                .box_shadows
                .iter()
                .map(|shadow| BoxShadow {
                    offset_x: shadow.offset_x.get(),
                    offset_y: shadow.offset_y.get(),
                    blur_radius: shadow.blur_radius.get(),
                    spread_radius: shadow.spread_radius.get(),
                    color: lower_color(&shadow.color),
                    inset: shadow.inset,
                })
                .collect(),
            clip_path: style.clip_path.as_ref().map(lower_clip_path),
            image_rendering: match style.image_rendering {
                ImageRenderingValue::Auto => ImageRendering::Auto,
                ImageRenderingValue::Pixelated => ImageRendering::Pixelated,
                ImageRenderingValue::CrispEdges => ImageRendering::CrispEdges,
            },
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

fn lower_clip_path(value: &ComputedClipPath) -> (PaintBox, ClipShape) {
    let reference_box = match value.reference_box {
        ClipBoxValue::Border => PaintBox::Border,
        ClipBoxValue::Padding => PaintBox::Padding,
        ClipBoxValue::Content => PaintBox::Content,
        ClipBoxValue::Fill => PaintBox::Fill,
        ClipBoxValue::Stroke => PaintBox::Stroke,
        ClipBoxValue::View => PaintBox::View,
    };
    let shape = match &value.shape {
        ComputedClipShape::Inset { offsets, radii } => ClipShape::Inset {
            edges: edges(offsets, coordinate),
            radii: PaintCorners {
                top_left: corner_radius(&radii.top_left),
                top_right: corner_radius(&radii.top_right),
                bottom_right: corner_radius(&radii.bottom_right),
                bottom_left: corner_radius(&radii.bottom_left),
            },
        },
        ComputedClipShape::Circle { radius, center } => ClipShape::Circle {
            radius: length(radius),
            center: clip_position(center),
        },
        ComputedClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => ClipShape::Ellipse {
            radius_x: length(radius_x),
            radius_y: length(radius_y),
            center: clip_position(center),
        },
        ComputedClipShape::Path {
            fill_rule,
            commands,
        } => ClipShape::Path {
            fill_rule: match fill_rule {
                ClipFillRuleValue::NonZero => FillRule::NonZero,
                ClipFillRuleValue::EvenOdd => FillRule::EvenOdd,
            },
            commands: commands
                .iter()
                .map(|command| match command {
                    ComputedClipPathCommand::MoveTo(value) => {
                        PathCommand::MoveTo(clip_position(value))
                    }
                    ComputedClipPathCommand::LineTo(value) => {
                        PathCommand::LineTo(clip_position(value))
                    }
                    ComputedClipPathCommand::QuadraticTo { control, end } => {
                        PathCommand::QuadraticTo {
                            control: clip_position(control),
                            end: clip_position(end),
                        }
                    }
                    ComputedClipPathCommand::CubicTo {
                        control_1,
                        control_2,
                        end,
                    } => PathCommand::CubicTo {
                        control_1: clip_position(control_1),
                        control_2: clip_position(control_2),
                        end: clip_position(end),
                    },
                    ComputedClipPathCommand::Close => PathCommand::Close,
                })
                .collect(),
        },
    };
    (reference_box, shape)
}

fn coordinate(value: &ComputedLengthPercentage) -> PaintCoordinate {
    PaintCoordinate {
        length: value.length(),
        fraction: value.fraction(),
    }
}

fn clip_position(value: &ComputedClipPoint) -> PaintPosition {
    PaintPosition {
        x: coordinate(&value.x),
        y: coordinate(&value.y),
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

    // Whisker applies `perspective` to the current node. Prepending it makes
    // the node's transform functions run
    // first and the perspective divide run afterward.
    let mut matrix = style
        .perspective
        .map(|distance| perspective(distance.get().max(1.0)))
        .unwrap_or_else(identity);
    let motion = match &style.offset_path {
        OffsetPathValue::None => None,
        path => Some(motion_path_state(
            path,
            style.offset_distance.get(),
            border_width,
            border_height,
        )?),
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
        ColorValue::Named(name) => named_color_srgb(name).map_or_else(
            || PaintColor::Named(name.clone()),
            |[red, green, blue]| PaintColor::Srgba {
                red,
                green,
                blue,
                alpha: 1.0,
            },
        ),
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
mod tests;
