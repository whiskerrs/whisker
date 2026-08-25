//! Lowering from computed style into the Host-independent paint protocol.

use whisker_protocol::{
    BorderLineStyle, BoxClip, BoxPaint, OverflowClip, PaintColor, PaintCornerRadius, PaintCorners,
    PaintEdges, PaintLengthPercentage, Transform, Visibility, VisualEffects,
};
use whisker_style::{
    BorderStyleValue, ColorValue, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedOffsetPathValue as OffsetPathValue, ComputedPaintStyle, ComputedTransformFunction,
    ComputedTransformStyle, MotionPathCommandValue, OffsetRotateValue, OverflowValue,
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

    // Lynx intentionally differs from browser CSS here: `perspective` affects
    // the current node. Prepending it makes the node's transform functions run
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

fn motion_path_state(
    path: &OffsetPathValue,
    progress: f32,
    border_width: f32,
    border_height: f32,
) -> Option<(f32, f32, f32)> {
    let mut segments = Vec::new();
    let mut total_length = 0.0_f32;
    match path {
        OffsetPathValue::None => return None,
        OffsetPathValue::Path(commands) => {
            let mut current = None;
            let mut subpath_start = None;
            for command in commands {
                let next = match *command {
                    MotionPathCommandValue::MoveTo(point) => {
                        let point = finite_motion_point(point)?;
                        current = Some(point);
                        subpath_start = Some(point);
                        continue;
                    }
                    MotionPathCommandValue::LineTo(point) => {
                        let next = finite_motion_point(point)?;
                        push_motion_line(&mut segments, current?, next);
                        next
                    }
                    MotionPathCommandValue::QuadraticTo { control, to } => {
                        let from = current?;
                        let control = finite_motion_point(control)?;
                        let to = finite_motion_point(to)?;
                        flatten_quadratic(&mut segments, from, control, to, 0);
                        to
                    }
                    MotionPathCommandValue::CubicTo {
                        control1,
                        control2,
                        to,
                    } => {
                        let from = current?;
                        let control1 = finite_motion_point(control1)?;
                        let control2 = finite_motion_point(control2)?;
                        let to = finite_motion_point(to)?;
                        flatten_cubic(&mut segments, from, control1, control2, to, 0);
                        to
                    }
                    MotionPathCommandValue::ArcTo {
                        radius_x,
                        radius_y,
                        x_axis_rotation,
                        large_arc,
                        sweep,
                        to,
                    } => {
                        let from = current?;
                        let to = finite_motion_point(to)?;
                        append_svg_arc(
                            &mut segments,
                            from,
                            to,
                            (radius_x.get(), radius_y.get()),
                            x_axis_rotation.get(),
                            large_arc,
                            sweep,
                        )?;
                        to
                    }
                    MotionPathCommandValue::Close => {
                        let next = subpath_start?;
                        push_motion_line(
                            &mut segments,
                            current.expect("a subpath start implies a current motion point"),
                            next,
                        );
                        next
                    }
                };
                current = Some(next);
            }
        }
        OffsetPathValue::Circle {
            radius,
            center_x,
            center_y,
        } => {
            let diagonal = border_width.hypot(border_height) / std::f32::consts::SQRT_2;
            let radius = resolve_motion_length(*radius, diagonal);
            let center = (
                resolve_motion_length(*center_x, border_width),
                resolve_motion_length(*center_y, border_height),
            );
            append_ellipse(
                &mut segments,
                center,
                (radius, radius),
                0.0,
                std::f32::consts::TAU,
            );
        }
        OffsetPathValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } => {
            let radii = (
                resolve_motion_length(*radius_x, border_width),
                resolve_motion_length(*radius_y, border_height),
            );
            let center = (
                resolve_motion_length(*center_x, border_width),
                resolve_motion_length(*center_y, border_height),
            );
            append_ellipse(&mut segments, center, radii, 0.0, std::f32::consts::TAU);
        }
        OffsetPathValue::Inset(value) => {
            append_inset_path(&mut segments, value, border_width, border_height)?;
        }
    }
    for segment in &segments {
        total_length += segment.length;
    }
    if segments.is_empty() || !total_length.is_finite() {
        return None;
    }
    let target = progress.clamp(0.0, 1.0) * total_length;
    let mut traversed = 0.0;
    let (last, preceding) = segments
        .split_last()
        .expect("motion path segments were checked as non-empty");
    for segment in preceding {
        if target <= traversed + segment.length {
            let local = ((target - traversed) / segment.length).clamp(0.0, 1.0);
            return Some(segment.point_and_tangent(local));
        }
        traversed += segment.length;
    }
    let local = ((target - traversed) / last.length).clamp(0.0, 1.0);
    Some(last.point_and_tangent(local))
}

type MotionPoint = (f32, f32);

const MOTION_CURVE_FLATNESS: f32 = 0.01;
const MOTION_CURVE_MAX_DEPTH: u8 = 10;

#[derive(Clone, Copy)]
enum MotionCurve {
    Line,
    Quadratic {
        start: MotionPoint,
        control: MotionPoint,
        end: MotionPoint,
    },
    Cubic {
        start: MotionPoint,
        control1: MotionPoint,
        control2: MotionPoint,
        end: MotionPoint,
    },
    Ellipse {
        center: MotionPoint,
        radii: MotionPoint,
        rotation: f32,
        start_angle: f32,
        end_angle: f32,
    },
}

#[derive(Clone, Copy)]
struct MotionSegment {
    from: MotionPoint,
    to: MotionPoint,
    length: f32,
    curve: MotionCurve,
}

impl MotionSegment {
    fn point_and_tangent(self, local: f32) -> (f32, f32, f32) {
        let (point, tangent) = match self.curve {
            MotionCurve::Line => (
                lerp_point(self.from, self.to, local),
                (self.to.0 - self.from.0, self.to.1 - self.from.1),
            ),
            MotionCurve::Quadratic {
                start,
                control,
                end,
            } => {
                let t = local;
                let one_minus_t = 1.0 - t;
                (
                    (
                        one_minus_t * one_minus_t * start.0
                            + 2.0 * one_minus_t * t * control.0
                            + t * t * end.0,
                        one_minus_t * one_minus_t * start.1
                            + 2.0 * one_minus_t * t * control.1
                            + t * t * end.1,
                    ),
                    (
                        2.0 * (one_minus_t * (control.0 - start.0) + t * (end.0 - control.0)),
                        2.0 * (one_minus_t * (control.1 - start.1) + t * (end.1 - control.1)),
                    ),
                )
            }
            MotionCurve::Cubic {
                start,
                control1,
                control2,
                end,
            } => {
                let t = local;
                let one_minus_t = 1.0 - t;
                (
                    (
                        one_minus_t.powi(3) * start.0
                            + 3.0 * one_minus_t * one_minus_t * t * control1.0
                            + 3.0 * one_minus_t * t * t * control2.0
                            + t.powi(3) * end.0,
                        one_minus_t.powi(3) * start.1
                            + 3.0 * one_minus_t * one_minus_t * t * control1.1
                            + 3.0 * one_minus_t * t * t * control2.1
                            + t.powi(3) * end.1,
                    ),
                    (
                        3.0 * (one_minus_t * one_minus_t * (control1.0 - start.0)
                            + 2.0 * one_minus_t * t * (control2.0 - control1.0)
                            + t * t * (end.0 - control2.0)),
                        3.0 * (one_minus_t * one_minus_t * (control1.1 - start.1)
                            + 2.0 * one_minus_t * t * (control2.1 - control1.1)
                            + t * t * (end.1 - control2.1)),
                    ),
                )
            }
            MotionCurve::Ellipse {
                center,
                radii,
                rotation,
                start_angle,
                end_angle,
            } => {
                let angle = start_angle + (end_angle - start_angle) * local;
                let local_tangent = (-radii.0 * angle.sin(), radii.1 * angle.cos());
                let (sin, cos) = rotation.sin_cos();
                (
                    ellipse_point(center, radii, angle, rotation),
                    (
                        cos * local_tangent.0 - sin * local_tangent.1,
                        sin * local_tangent.0 + cos * local_tangent.1,
                    ),
                )
            }
        };
        let tangent = if tangent.0.hypot(tangent.1) > f32::EPSILON {
            tangent
        } else {
            (self.to.0 - self.from.0, self.to.1 - self.from.1)
        };
        let angle = tangent.1.atan2(tangent.0).to_degrees().rem_euclid(360.0);
        (point.0, point.1, angle)
    }
}

fn finite_motion_point(value: whisker_style::MotionPathPointValue) -> Option<MotionPoint> {
    let point = (value.x.get(), value.y.get());
    (point.0.is_finite() && point.1.is_finite()).then_some(point)
}

fn push_motion_line(segments: &mut Vec<MotionSegment>, from: MotionPoint, to: MotionPoint) {
    push_motion_segment(segments, from, to, MotionCurve::Line);
}

fn resolve_motion_length(value: whisker_style::ComputedLengthPercentage, context: f32) -> f32 {
    value.length() + value.fraction() * context
}

fn append_inset_path(
    segments: &mut Vec<MotionSegment>,
    value: &whisker_style::ComputedInsetPathValue,
    border_width: f32,
    border_height: f32,
) -> Option<()> {
    let mut top = resolve_motion_length(value.offsets.top, border_height);
    let mut right = resolve_motion_length(value.offsets.right, border_width);
    let mut bottom = resolve_motion_length(value.offsets.bottom, border_height);
    let mut left = resolve_motion_length(value.offsets.left, border_width);
    if ![top, right, bottom, left]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }

    let vertical_inset = f64::from(top) + f64::from(bottom);
    if vertical_inset > f64::from(border_height) {
        let scale = f64::from(border_height) / vertical_inset;
        top = (f64::from(top) * scale) as f32;
        bottom = (f64::from(bottom) * scale) as f32;
    }
    let horizontal_inset = f64::from(left) + f64::from(right);
    if horizontal_inset > f64::from(border_width) {
        let scale = f64::from(border_width) / horizontal_inset;
        left = (f64::from(left) * scale) as f32;
        right = (f64::from(right) * scale) as f32;
    }

    let width = (border_width - left - right).max(0.0);
    let height = (border_height - top - bottom).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let right_edge = left + width;
    let bottom_edge = top + height;
    let mut radii = [[0.0_f32; 2]; 4];
    if let Some(computed) = &value.radii {
        let corners = [
            computed.top_left,
            computed.top_right,
            computed.bottom_right,
            computed.bottom_left,
        ];
        for (resolved, corner) in radii.iter_mut().zip(corners) {
            resolved[0] = resolve_motion_length(corner.horizontal, width);
            resolved[1] = resolve_motion_length(corner.vertical, height);
            if !resolved[0].is_finite()
                || !resolved[1].is_finite()
                || resolved[0] < 0.0
                || resolved[1] < 0.0
            {
                return None;
            }
            if resolved[0] <= 0.0 || resolved[1] <= 0.0 {
                *resolved = [0.0, 0.0];
            }
        }
    }

    let mut radius_scale = 1.0_f64;
    for (sum, side) in [
        (
            f64::from(radii[0][0]) + f64::from(radii[1][0]),
            f64::from(width),
        ),
        (
            f64::from(radii[3][0]) + f64::from(radii[2][0]),
            f64::from(width),
        ),
        (
            f64::from(radii[0][1]) + f64::from(radii[3][1]),
            f64::from(height),
        ),
        (
            f64::from(radii[1][1]) + f64::from(radii[2][1]),
            f64::from(height),
        ),
    ] {
        if sum > side {
            radius_scale = radius_scale.min(side / sum);
        }
    }
    if radius_scale < 1.0 {
        for radius in &mut radii {
            radius[0] = (f64::from(radius[0]) * radius_scale) as f32;
            radius[1] = (f64::from(radius[1]) * radius_scale) as f32;
        }
    }

    let [top_left, top_right, bottom_right, bottom_left] = radii;
    let start = (left + top_left[0], top);
    let mut current = start;

    let next = (right_edge - top_right[0], top);
    push_motion_line(segments, current, next);
    current = next;
    if top_right[0] > 0.0 {
        append_ellipse(
            segments,
            (right_edge - top_right[0], top + top_right[1]),
            (top_right[0], top_right[1]),
            -std::f32::consts::FRAC_PI_2,
            0.0,
        );
        current = (right_edge, top + top_right[1]);
    }

    let next = (right_edge, bottom_edge - bottom_right[1]);
    push_motion_line(segments, current, next);
    current = next;
    if bottom_right[0] > 0.0 {
        append_ellipse(
            segments,
            (right_edge - bottom_right[0], bottom_edge - bottom_right[1]),
            (bottom_right[0], bottom_right[1]),
            0.0,
            std::f32::consts::FRAC_PI_2,
        );
        current = (right_edge - bottom_right[0], bottom_edge);
    }

    let next = (left + bottom_left[0], bottom_edge);
    push_motion_line(segments, current, next);
    current = next;
    if bottom_left[0] > 0.0 {
        append_ellipse(
            segments,
            (left + bottom_left[0], bottom_edge - bottom_left[1]),
            (bottom_left[0], bottom_left[1]),
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
        );
        current = (left, bottom_edge - bottom_left[1]);
    }

    let next = (left, top + top_left[1]);
    push_motion_line(segments, current, next);
    current = next;
    if top_left[0] > 0.0 {
        append_ellipse(
            segments,
            (left + top_left[0], top + top_left[1]),
            (top_left[0], top_left[1]),
            std::f32::consts::PI,
            std::f32::consts::PI * 1.5,
        );
        current = start;
    }
    push_motion_line(segments, current, start);
    Some(())
}

fn push_motion_segment(
    segments: &mut Vec<MotionSegment>,
    from: MotionPoint,
    to: MotionPoint,
    curve: MotionCurve,
) {
    let length = (to.0 - from.0).hypot(to.1 - from.1);
    if length > 0.0 && length.is_finite() {
        segments.push(MotionSegment {
            from,
            to,
            length,
            curve,
        });
    }
}

fn flatten_quadratic(
    segments: &mut Vec<MotionSegment>,
    start: MotionPoint,
    control: MotionPoint,
    end: MotionPoint,
    depth: u8,
) {
    if depth == MOTION_CURVE_MAX_DEPTH
        || point_line_distance(control, start, end) <= MOTION_CURVE_FLATNESS
    {
        push_motion_segment(
            segments,
            start,
            end,
            MotionCurve::Quadratic {
                start,
                control,
                end,
            },
        );
        return;
    }
    let start_control = midpoint(start, control);
    let control_end = midpoint(control, end);
    let middle = midpoint(start_control, control_end);
    flatten_quadratic(segments, start, start_control, middle, depth + 1);
    flatten_quadratic(segments, middle, control_end, end, depth + 1);
}

fn flatten_cubic(
    segments: &mut Vec<MotionSegment>,
    start: MotionPoint,
    control1: MotionPoint,
    control2: MotionPoint,
    end: MotionPoint,
    depth: u8,
) {
    if depth == MOTION_CURVE_MAX_DEPTH
        || point_line_distance(control1, start, end).max(point_line_distance(control2, start, end))
            <= MOTION_CURVE_FLATNESS
    {
        push_motion_segment(
            segments,
            start,
            end,
            MotionCurve::Cubic {
                start,
                control1,
                control2,
                end,
            },
        );
        return;
    }
    let p01 = midpoint(start, control1);
    let p12 = midpoint(control1, control2);
    let p23 = midpoint(control2, end);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let middle = midpoint(p012, p123);
    flatten_cubic(segments, start, p01, p012, middle, depth + 1);
    flatten_cubic(segments, middle, p123, p23, end, depth + 1);
}

fn append_svg_arc(
    segments: &mut Vec<MotionSegment>,
    from: MotionPoint,
    to: MotionPoint,
    radii: MotionPoint,
    rotation_degrees: f32,
    large_arc: bool,
    sweep: bool,
) -> Option<()> {
    if !radii.0.is_finite() || !radii.1.is_finite() || !rotation_degrees.is_finite() {
        return None;
    }
    if from == to {
        return Some(());
    }
    let mut radius_x = f64::from(radii.0.abs());
    let mut radius_y = f64::from(radii.1.abs());
    if radius_x == 0.0 || radius_y == 0.0 {
        push_motion_line(segments, from, to);
        return Some(());
    }

    let rotation = f64::from(rotation_degrees.rem_euclid(360.0)).to_radians();
    let (sin_rotation, cos_rotation) = rotation.sin_cos();
    let half_delta = (
        (f64::from(from.0) - f64::from(to.0)) * 0.5,
        (f64::from(from.1) - f64::from(to.1)) * 0.5,
    );
    let transformed = (
        cos_rotation * half_delta.0 + sin_rotation * half_delta.1,
        -sin_rotation * half_delta.0 + cos_rotation * half_delta.1,
    );

    let mut radius_x_squared = radius_x * radius_x;
    let mut radius_y_squared = radius_y * radius_y;
    let transformed_x_squared = transformed.0 * transformed.0;
    let transformed_y_squared = transformed.1 * transformed.1;
    let size_ratio =
        transformed_x_squared / radius_x_squared + transformed_y_squared / radius_y_squared;
    if size_ratio > 1.0 {
        let scale = size_ratio.sqrt();
        radius_x *= scale;
        radius_y *= scale;
        radius_x_squared = radius_x * radius_x;
        radius_y_squared = radius_y * radius_y;
    }

    let numerator = (radius_x_squared * radius_y_squared
        - radius_x_squared * transformed_y_squared
        - radius_y_squared * transformed_x_squared)
        .max(0.0);
    // Distinct endpoints and non-zero finite f32 radii make this denominator
    // strictly positive and finite in f64.
    let denominator =
        radius_x_squared * transformed_y_squared + radius_y_squared * transformed_x_squared;
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let factor = sign * (numerator / denominator).sqrt();
    let transformed_center = (
        factor * radius_x * transformed.1 / radius_y,
        factor * -radius_y * transformed.0 / radius_x,
    );
    let midpoint = (
        (f64::from(from.0) + f64::from(to.0)) * 0.5,
        (f64::from(from.1) + f64::from(to.1)) * 0.5,
    );
    let center = (
        cos_rotation * transformed_center.0 - sin_rotation * transformed_center.1 + midpoint.0,
        sin_rotation * transformed_center.0 + cos_rotation * transformed_center.1 + midpoint.1,
    );

    let start_vector = (
        (transformed.0 - transformed_center.0) / radius_x,
        (transformed.1 - transformed_center.1) / radius_y,
    );
    let end_vector = (
        (-transformed.0 - transformed_center.0) / radius_x,
        (-transformed.1 - transformed_center.1) / radius_y,
    );
    let start_angle = start_vector.1.atan2(start_vector.0);
    let mut sweep_angle = (start_vector.0 * end_vector.1 - start_vector.1 * end_vector.0)
        .atan2(start_vector.0 * end_vector.0 + start_vector.1 * end_vector.1);
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= std::f64::consts::TAU;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += std::f64::consts::TAU;
    }

    let resolved = [
        center.0 as f32,
        center.1 as f32,
        radius_x as f32,
        radius_y as f32,
        rotation as f32,
        start_angle as f32,
        (start_angle + sweep_angle) as f32,
    ];
    if !resolved.iter().all(|value| value.is_finite()) {
        return None;
    }
    append_rotated_ellipse(
        segments,
        (resolved[0], resolved[1]),
        (resolved[2], resolved[3]),
        resolved[4],
        resolved[5],
        resolved[6],
    );
    Some(())
}

fn append_ellipse(
    segments: &mut Vec<MotionSegment>,
    center: MotionPoint,
    radii: MotionPoint,
    start_angle: f32,
    end_angle: f32,
) {
    append_rotated_ellipse(segments, center, radii, 0.0, start_angle, end_angle);
}

fn append_rotated_ellipse(
    segments: &mut Vec<MotionSegment>,
    center: MotionPoint,
    radii: MotionPoint,
    rotation: f32,
    start_angle: f32,
    end_angle: f32,
) {
    if !center.0.is_finite()
        || !center.1.is_finite()
        || !radii.0.is_finite()
        || !radii.1.is_finite()
        || !rotation.is_finite()
        || radii.0 <= 0.0
        || radii.1 <= 0.0
    {
        return;
    }
    flatten_ellipse(segments, center, radii, rotation, start_angle, end_angle, 0);
}

fn flatten_ellipse(
    segments: &mut Vec<MotionSegment>,
    center: MotionPoint,
    radii: MotionPoint,
    rotation: f32,
    start_angle: f32,
    end_angle: f32,
    depth: u8,
) {
    let start = ellipse_point(center, radii, start_angle, rotation);
    let end = ellipse_point(center, radii, end_angle, rotation);
    let middle_angle = (start_angle + end_angle) * 0.5;
    let middle = ellipse_point(center, radii, middle_angle, rotation);
    if depth == MOTION_CURVE_MAX_DEPTH
        || point_line_distance(middle, start, end) <= MOTION_CURVE_FLATNESS
    {
        push_motion_segment(
            segments,
            start,
            end,
            MotionCurve::Ellipse {
                center,
                radii,
                rotation,
                start_angle,
                end_angle,
            },
        );
        return;
    }
    flatten_ellipse(
        segments,
        center,
        radii,
        rotation,
        start_angle,
        middle_angle,
        depth + 1,
    );
    flatten_ellipse(
        segments,
        center,
        radii,
        rotation,
        middle_angle,
        end_angle,
        depth + 1,
    );
}

fn ellipse_point(
    center: MotionPoint,
    radii: MotionPoint,
    angle: f32,
    rotation: f32,
) -> MotionPoint {
    let local = (radii.0 * angle.cos(), radii.1 * angle.sin());
    let (sin, cos) = rotation.sin_cos();
    (
        center.0 + cos * local.0 - sin * local.1,
        center.1 + sin * local.0 + cos * local.1,
    )
}

fn point_line_distance(point: MotionPoint, start: MotionPoint, end: MotionPoint) -> f32 {
    let delta = (end.0 - start.0, end.1 - start.1);
    let chord = delta.0.hypot(delta.1);
    if chord <= f32::EPSILON {
        return (point.0 - start.0).hypot(point.1 - start.1);
    }
    ((point.0 - start.0) * delta.1 - (point.1 - start.1) * delta.0).abs() / chord
}

fn midpoint(a: MotionPoint, b: MotionPoint) -> MotionPoint {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

fn lerp_point(a: MotionPoint, b: MotionPoint, t: f32) -> MotionPoint {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
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
        assert_eq!(
            motion_path_state(&OffsetPathValue::None, 0.5, 1.0, 1.0),
            None
        );
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
            OffsetPathValue::Path(vec![MotionPathCommandValue::MoveTo(point(f32::NAN, 0.0))]),
            OffsetPathValue::Path(vec![MotionPathCommandValue::LineTo(point(1.0, 0.0))]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::LineTo(point(f32::NAN, 0.0)),
            ]),
            OffsetPathValue::Path(vec![MotionPathCommandValue::QuadraticTo {
                control: point(1.0, 0.0),
                to: point(2.0, 0.0),
            }]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::QuadraticTo {
                    control: point(f32::NAN, 0.0),
                    to: point(2.0, 0.0),
                },
            ]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::QuadraticTo {
                    control: point(1.0, 0.0),
                    to: point(f32::NAN, 0.0),
                },
            ]),
            OffsetPathValue::Path(vec![MotionPathCommandValue::CubicTo {
                control1: point(1.0, 0.0),
                control2: point(2.0, 0.0),
                to: point(3.0, 0.0),
            }]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::CubicTo {
                    control1: point(f32::NAN, 0.0),
                    control2: point(2.0, 0.0),
                    to: point(3.0, 0.0),
                },
            ]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::CubicTo {
                    control1: point(1.0, 0.0),
                    control2: point(f32::NAN, 0.0),
                    to: point(3.0, 0.0),
                },
            ]),
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
                MotionPathCommandValue::CubicTo {
                    control1: point(1.0, 0.0),
                    control2: point(2.0, 0.0),
                    to: point(f32::NAN, 0.0),
                },
            ]),
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

        let quadratic = OffsetPathValue::Path(vec![
            MotionPathCommandValue::MoveTo(point(0.0, 20.0)),
            MotionPathCommandValue::QuadraticTo {
                control: point(0.0, 0.0),
                to: point(20.0, 0.0),
            },
        ]);
        let (x, y, angle) = motion_path_state(&quadratic, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 5.0).abs() < 0.001, "{x}");
        assert!((y - 5.0).abs() < 0.001, "{y}");
        assert!((angle - 315.0).abs() < 0.001, "{angle}");

        let cubic = OffsetPathValue::Path(vec![
            MotionPathCommandValue::MoveTo(point(20.0, 0.0)),
            MotionPathCommandValue::CubicTo {
                control1: point(0.0, 0.0),
                control2: point(0.0, 20.0),
                to: point(20.0, 20.0),
            },
        ]);
        let (x, y, angle) = motion_path_state(&cubic, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 5.0).abs() < 0.001, "{x}");
        assert!((y - 10.0).abs() < 0.001, "{y}");
        assert!((angle - 90.0).abs() < 0.001, "{angle}");

        let arc = |from, to, radius_x, radius_y, rotation, large_arc, sweep| {
            OffsetPathValue::Path(vec![
                MotionPathCommandValue::MoveTo(from),
                MotionPathCommandValue::ArcTo {
                    radius_x: StyleNumber::new(radius_x),
                    radius_y: StyleNumber::new(radius_y),
                    x_axis_rotation: StyleNumber::new(rotation),
                    large_arc,
                    sweep,
                    to,
                },
            ])
        };
        let upper_arc = arc(
            point(0.0, 0.0),
            point(100.0, 0.0),
            50.0,
            50.0,
            0.0,
            false,
            true,
        );
        let (x, y, angle) = motion_path_state(&upper_arc, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 50.0).abs() < 0.001, "{x}");
        assert!((y + 50.0).abs() < 0.001, "{y}");
        assert!(angle.abs() < 0.001, "{angle}");

        let lower_arc = arc(
            point(0.0, 0.0),
            point(100.0, 0.0),
            50.0,
            50.0,
            0.0,
            false,
            false,
        );
        let (x, y, angle) = motion_path_state(&lower_arc, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 50.0).abs() < 0.001, "{x}");
        assert!((y - 50.0).abs() < 0.001, "{y}");
        assert!((angle - 180.0).abs() < 0.001, "{angle}");

        let rotated_arc = arc(
            point(0.0, -50.0),
            point(0.0, 50.0),
            50.0,
            20.0,
            90.0,
            false,
            true,
        );
        let (x, y, angle) = motion_path_state(&rotated_arc, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 20.0).abs() < 0.001, "{x}");
        assert!(y.abs() < 0.001, "{y}");
        assert!((angle - 90.0).abs() < 0.001, "{angle}");

        let corrected_arc = arc(
            point(0.0, 0.0),
            point(100.0, 0.0),
            -10.0,
            -10.0,
            0.0,
            false,
            true,
        );
        let (x, y, _) = motion_path_state(&corrected_arc, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 50.0).abs() < 0.001, "{x}");
        assert!((y + 50.0).abs() < 0.001, "{y}");

        let large_arc = arc(
            point(0.0, 0.0),
            point(80.0, 0.0),
            50.0,
            50.0,
            0.0,
            true,
            true,
        );
        let (x, y, angle) = motion_path_state(&large_arc, 0.5, 1.0, 1.0).unwrap();
        assert!((x - 40.0).abs() < 0.001, "{x}");
        assert!((y + 80.0).abs() < 0.001, "{y}");
        assert!(angle.abs().min((angle - 360.0).abs()) < 0.001, "{angle}");

        let reverse_large_arc = arc(
            point(0.0, 0.0),
            point(80.0, 0.0),
            50.0,
            50.0,
            0.0,
            true,
            false,
        );
        assert!(motion_path_state(&reverse_large_arc, 0.5, 1.0, 1.0).is_some());

        let line_arc = arc(
            point(0.0, 0.0),
            point(100.0, 0.0),
            0.0,
            20.0,
            0.0,
            false,
            true,
        );
        let (x, y, angle) = motion_path_state(&line_arc, 0.5, 1.0, 1.0).unwrap();
        assert_eq!((x, y, angle), (50.0, 0.0, 0.0));

        let other_line_arc = arc(
            point(0.0, 0.0),
            point(100.0, 0.0),
            20.0,
            0.0,
            0.0,
            false,
            true,
        );
        let (x, y, angle) = motion_path_state(&other_line_arc, 0.5, 1.0, 1.0).unwrap();
        assert_eq!((x, y, angle), (50.0, 0.0, 0.0));

        let omitted_arc = OffsetPathValue::Path(vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: StyleNumber::new(10.0),
                radius_y: StyleNumber::new(10.0),
                x_axis_rotation: StyleNumber::new(0.0),
                large_arc: true,
                sweep: true,
                to: point(0.0, 0.0),
            },
            MotionPathCommandValue::LineTo(point(100.0, 0.0)),
        ]);
        let (x, y, angle) = motion_path_state(&omitted_arc, 0.5, 1.0, 1.0).unwrap();
        assert_eq!((x, y, angle), (50.0, 0.0, 0.0));

        for invalid_arc in [
            OffsetPathValue::Path(vec![MotionPathCommandValue::ArcTo {
                radius_x: StyleNumber::new(1.0),
                radius_y: StyleNumber::new(1.0),
                x_axis_rotation: StyleNumber::new(0.0),
                large_arc: false,
                sweep: true,
                to: point(1.0, 0.0),
            }]),
            arc(
                point(0.0, 0.0),
                point(f32::NAN, 0.0),
                1.0,
                1.0,
                0.0,
                false,
                true,
            ),
            arc(
                point(0.0, 0.0),
                point(1.0, 0.0),
                f32::NAN,
                1.0,
                0.0,
                false,
                true,
            ),
            arc(
                point(0.0, 0.0),
                point(1.0, 0.0),
                1.0,
                f32::NAN,
                0.0,
                false,
                true,
            ),
            arc(
                point(0.0, 0.0),
                point(1.0, 0.0),
                1.0,
                1.0,
                f32::NAN,
                false,
                true,
            ),
            arc(
                point(f32::MAX, -1.0),
                point(f32::MAX, 1.0),
                f32::MAX,
                f32::MAX,
                0.0,
                true,
                true,
            ),
        ] {
            assert_eq!(motion_path_state(&invalid_arc, 0.5, 1.0, 1.0), None);
        }

        let mut invalid_rotated_segments = Vec::new();
        append_rotated_ellipse(
            &mut invalid_rotated_segments,
            (0.0, 0.0),
            (10.0, 5.0),
            f32::NAN,
            0.0,
            std::f32::consts::TAU,
        );
        assert!(invalid_rotated_segments.is_empty());

        let circle = OffsetPathValue::Circle {
            radius: ComputedLengthPercentage::new(0.0, 0.5),
            center_x: ComputedLengthPercentage::new(0.0, 0.5),
            center_y: ComputedLengthPercentage::new(0.0, 0.5),
        };
        let (x, y, angle) = motion_path_state(&circle, 0.25, 40.0, 20.0).unwrap();
        assert!((x - 20.0).abs() < 0.001, "{x}");
        assert!((y - 25.811_388).abs() < 0.001, "{y}");
        assert!((angle - 180.0).abs() < 0.001, "{angle}");

        let ellipse = OffsetPathValue::Ellipse {
            radius_x: ComputedLengthPercentage::new(0.0, 0.25),
            radius_y: ComputedLengthPercentage::new(0.0, 0.25),
            center_x: ComputedLengthPercentage::new(0.0, 0.5),
            center_y: ComputedLengthPercentage::new(0.0, 0.5),
        };
        let (x, y, angle) = motion_path_state(&ellipse, 0.25, 40.0, 20.0).unwrap();
        assert!((x - 20.0).abs() < 0.001, "{x}");
        assert!((y - 15.0).abs() < 0.001, "{y}");
        assert!((angle - 180.0).abs() < 0.001, "{angle}");

        let zero = ComputedLengthPercentage::ZERO;
        let ten = ComputedLengthPercentage::new(10.0, 0.0);
        let inset = OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
            offsets: whisker_style::Edges {
                top: ten,
                right: ten,
                bottom: ten,
                left: ten,
            },
            radii: None,
        }));
        let (x, y, angle) = motion_path_state(&inset, 0.25, 100.0, 60.0).unwrap();
        assert!((x - 70.0).abs() < 0.001, "{x}");
        assert!((y - 10.0).abs() < 0.001, "{y}");
        assert!(angle.abs() < 0.001, "{angle}");

        let radius = whisker_style::ComputedCornerRadius {
            horizontal: ten,
            vertical: ComputedLengthPercentage::new(5.0, 0.0),
        };
        let rounded = OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
            offsets: whisker_style::Edges {
                top: ten,
                right: ten,
                bottom: ten,
                left: ten,
            },
            radii: Some(whisker_style::Corners {
                top_left: radius,
                top_right: radius,
                bottom_right: radius,
                bottom_left: radius,
            }),
        }));
        let (x, y, angle) = motion_path_state(&rounded, 0.5, 100.0, 60.0).unwrap();
        assert!((x - 80.0).abs() < 0.001, "{x}");
        assert!((y - 50.0).abs() < 0.001, "{y}");
        assert!((angle - 180.0).abs() < 0.001, "{angle}");

        assert_eq!(
            motion_path_state(
                &OffsetPathValue::Ellipse {
                    radius_x: ComputedLengthPercentage::ZERO,
                    radius_y: ComputedLengthPercentage::new(1.0, 0.0),
                    center_x: ComputedLengthPercentage::ZERO,
                    center_y: ComputedLengthPercentage::ZERO,
                },
                0.5,
                40.0,
                20.0,
            ),
            None
        );
        let collapsed = OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
            offsets: whisker_style::Edges {
                top: zero,
                right: ComputedLengthPercentage::new(100.0, 0.0),
                bottom: zero,
                left: ComputedLengthPercentage::new(100.0, 0.0),
            },
            radii: None,
        }));
        assert_eq!(motion_path_state(&collapsed, 0.0, 100.0, 60.0), None);

        let invalid_offset =
            OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
                offsets: whisker_style::Edges {
                    top: ComputedLengthPercentage::new(f32::MAX, f32::MAX),
                    right: zero,
                    bottom: zero,
                    left: zero,
                },
                radii: None,
            }));
        assert_eq!(motion_path_state(&invalid_offset, 0.0, 100.0, 60.0), None);

        let vertically_collapsed =
            OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
                offsets: whisker_style::Edges {
                    top: ComputedLengthPercentage::new(40.0, 0.0),
                    right: zero,
                    bottom: ComputedLengthPercentage::new(40.0, 0.0),
                    left: zero,
                },
                radii: None,
            }));
        assert_eq!(
            motion_path_state(&vertically_collapsed, 0.0, 100.0, 60.0),
            None
        );

        let corner = |horizontal, vertical| whisker_style::ComputedCornerRadius {
            horizontal: ComputedLengthPercentage::new(horizontal, 0.0),
            vertical: ComputedLengthPercentage::new(vertical, 0.0),
        };
        let inset_with_radius = |radius| {
            OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
                offsets: whisker_style::Edges {
                    top: ten,
                    right: ten,
                    bottom: ten,
                    left: ten,
                },
                radii: Some(whisker_style::Corners {
                    top_left: radius,
                    top_right: radius,
                    bottom_right: radius,
                    bottom_left: radius,
                }),
            }))
        };
        assert_eq!(
            motion_path_state(&inset_with_radius(corner(f32::NAN, 1.0)), 0.0, 100.0, 60.0,),
            None
        );
        let (x, y, _) =
            motion_path_state(&inset_with_radius(corner(10.0, 0.0)), 0.0, 100.0, 60.0).unwrap();
        assert_eq!((x, y), (10.0, 10.0));
        let (x, y, _) =
            motion_path_state(&inset_with_radius(corner(100.0, 100.0)), 0.0, 100.0, 60.0).unwrap();
        assert!((x - 30.0).abs() < 0.001, "{x}");
        assert!((y - 10.0).abs() < 0.001, "{y}");

        assert_eq!(point_line_distance((2.0, 0.0), (1.0, 0.0), (1.0, 0.0)), 1.0);
        let cusp = MotionSegment {
            from: (0.0, 0.0),
            to: (2.0, 0.0),
            length: 2.0,
            curve: MotionCurve::Cubic {
                start: (0.0, 0.0),
                control1: (1.0, 0.0),
                control2: (-1.0, 0.0),
                end: (2.0, 0.0),
            },
        };
        assert_eq!(cusp.point_and_tangent(0.5).2, 0.0);
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
