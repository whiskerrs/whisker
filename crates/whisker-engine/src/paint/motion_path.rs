use super::*;

pub(super) fn motion_path_state(
    path: &OffsetPathValue,
    progress: f32,
    border_width: f32,
    border_height: f32,
) -> Option<(f32, f32, f32)> {
    let mut segments = Vec::new();
    let mut total_length = 0.0_f32;
    let mut zero_length_position = None;
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
                        zero_length_position.get_or_insert(point);
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
            if ![radius, center.0, center.1].into_iter().all(f32::is_finite) || radius < 0.0 {
                return None;
            }
            zero_length_position = Some((center.0 + radius, center.1));
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
            if ![radii.0, radii.1, center.0, center.1]
                .into_iter()
                .all(f32::is_finite)
                || radii.0 < 0.0
                || radii.1 < 0.0
            {
                return None;
            }
            zero_length_position = Some((center.0 + radii.0, center.1));
            append_ellipse(&mut segments, center, radii, 0.0, std::f32::consts::TAU);
        }
        OffsetPathValue::Inset(value) => {
            zero_length_position = Some(append_inset_path(
                &mut segments,
                value,
                border_width,
                border_height,
            )?);
        }
    }
    for segment in &segments {
        total_length += segment.length;
    }
    if !total_length.is_finite() {
        return None;
    }
    if segments.is_empty() {
        return zero_length_position.map(|point| (point.0, point.1, 0.0));
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

pub(super) type MotionPoint = (f32, f32);

const MOTION_CURVE_FLATNESS: f32 = 0.01;
const MOTION_CURVE_MAX_DEPTH: u8 = 10;

#[derive(Clone, Copy)]
pub(super) enum MotionCurve {
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
pub(super) struct MotionSegment {
    pub(super) from: MotionPoint,
    pub(super) to: MotionPoint,
    pub(super) length: f32,
    pub(super) curve: MotionCurve,
}

impl MotionSegment {
    pub(super) fn point_and_tangent(self, local: f32) -> (f32, f32, f32) {
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
) -> Option<MotionPoint> {
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
        return Some((left, top));
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
    Some(start)
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

pub(super) fn append_rotated_ellipse(
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

pub(super) fn point_line_distance(point: MotionPoint, start: MotionPoint, end: MotionPoint) -> f32 {
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
