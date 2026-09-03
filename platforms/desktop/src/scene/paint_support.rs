use super::*;

pub(super) fn supports_basic_background_layer(layer: &BackgroundLayer) -> bool {
    let resource_image = matches!(&layer.image, PaintImage::Resource(_));
    let supported_image = matches!(
        &layer.image,
        PaintImage::LinearGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| stop.position.is_some())
    ) || matches!(
        &layer.image,
        PaintImage::RadialGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| stop.position.is_some())
    ) || matches!(
        &layer.image,
        PaintImage::ConicGradient {
            repeating: false,
            stops,
            ..
        } if stops.iter().all(|stop| {
            stop.position.is_some_and(|position| position.length == 0.0)
        })
    ) || resource_image;
    let initial_geometry = layer.position == Default::default()
        && layer.size == BackgroundSize::Auto
        && layer.repeat_x == ImageRepeat::Repeat
        && layer.repeat_y == ImageRepeat::Repeat
        && layer.origin == PaintBox::Padding
        && layer.clip == PaintBox::Border;
    let supported_geometry = match layer.size {
        BackgroundSize::Auto => resource_image || initial_geometry,
        BackgroundSize::Cover | BackgroundSize::Contain => resource_image,
        BackgroundSize::Explicit { width, height } => {
            width.is_some() && height.is_some()
                || resource_image && (width.is_some() || height.is_some())
        }
    };
    supported_image
        && supported_geometry
        && layer.attachment == BackgroundAttachment::Scroll
        && layer.blend_mode == BlendMode::Normal
}

pub(super) fn supports_visual_effects(effects: &VisualEffects) -> bool {
    let mut remainder = effects.clone();
    remainder.box_shadows.clear();
    remainder.clip_path = None;
    remainder.backdrop_blur = None;
    remainder.image_rendering = whisker_protocol::ImageRendering::Auto;
    remainder == VisualEffects::default()
        && matches!(
            effects.image_rendering,
            whisker_protocol::ImageRendering::Auto
                | whisker_protocol::ImageRendering::Pixelated
                | whisker_protocol::ImageRendering::CrispEdges
        )
        && effects.clip_path.as_ref().is_none_or(|(reference, shape)| {
            matches!(
                reference,
                PaintBox::Border | PaintBox::Padding | PaintBox::Content
            ) && matches!(
                shape,
                ClipShape::Inset { .. }
                    | ClipShape::Circle { .. }
                    | ClipShape::Ellipse { .. }
                    | ClipShape::Path { .. }
            )
        })
}

pub(super) fn clip_shape_geometry(
    reference: LayoutRect,
    shape: &ClipShape,
) -> (
    LayoutRect,
    ResolvedRadii,
    Option<Arc<[PathSegment]>>,
    FillRule,
) {
    let zero_radii = || ResolvedRadii {
        horizontal: [0.0; 4],
        vertical: [0.0; 4],
    };
    match shape {
        ClipShape::Inset { edges, radii } => {
            let rect = inset_clip_rect(reference, edges);
            let radii = resolve_radii(radii, rect);
            (rect, radii, None, FillRule::NonZero)
        }
        ClipShape::Circle { radius, center } => {
            let center_x = reference.x + resolve_coordinate(center.x, reference.width);
            let center_y = reference.y + resolve_coordinate(center.y, reference.height);
            let normalized_diagonal = reference.width.hypot(reference.height) / 2.0_f32.sqrt();
            let radius = resolve_length_percentage(*radius, normalized_diagonal);
            let rect = LayoutRect {
                x: center_x - radius,
                y: center_y - radius,
                width: radius * 2.0,
                height: radius * 2.0,
            };
            (
                rect,
                ResolvedRadii {
                    horizontal: [radius; 4],
                    vertical: [radius; 4],
                },
                None,
                FillRule::NonZero,
            )
        }
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => {
            let center_x = reference.x + resolve_coordinate(center.x, reference.width);
            let center_y = reference.y + resolve_coordinate(center.y, reference.height);
            let radius_x = resolve_length_percentage(*radius_x, reference.width);
            let radius_y = resolve_length_percentage(*radius_y, reference.height);
            (
                LayoutRect {
                    x: center_x - radius_x,
                    y: center_y - radius_y,
                    width: radius_x * 2.0,
                    height: radius_y * 2.0,
                },
                ResolvedRadii {
                    horizontal: [radius_x; 4],
                    vertical: [radius_y; 4],
                },
                None,
                FillRule::NonZero,
            )
        }
        ClipShape::Path {
            fill_rule,
            commands,
        } => {
            let segments = flatten_path(reference, commands);
            let bounds = path_bounds(&segments).unwrap_or(reference);
            (bounds, zero_radii(), Some(segments.into()), *fill_rule)
        }
        _ => unreachable!("unsupported clip-path shape passed validation"),
    }
}

pub(super) fn resolve_path_position(reference: LayoutRect, position: PaintPosition) -> [f32; 2] {
    [
        reference.x + resolve_coordinate(position.x, reference.width),
        reference.y + resolve_coordinate(position.y, reference.height),
    ]
}

pub(super) fn add_path_segment(segments: &mut Vec<PathSegment>, from: [f32; 2], to: [f32; 2]) {
    if from != to {
        segments.push(PathSegment { from, to });
    }
}

pub(super) fn close_path_subpath(
    segments: &mut Vec<PathSegment>,
    current: &mut Option<[f32; 2]>,
    start: Option<[f32; 2]>,
) {
    if let (Some(from), Some(to)) = (*current, start) {
        add_path_segment(segments, from, to);
        *current = Some(to);
    }
}

pub(super) fn flatten_path(reference: LayoutRect, commands: &[PathCommand]) -> Vec<PathSegment> {
    const CURVE_STEPS: usize = 16;
    let mut segments = Vec::new();
    let mut current = None;
    let mut start = None;
    for command in commands {
        match command {
            PathCommand::MoveTo(point) => {
                close_path_subpath(&mut segments, &mut current, start);
                let point = resolve_path_position(reference, *point);
                current = Some(point);
                start = Some(point);
            }
            PathCommand::LineTo(point) => {
                let to = resolve_path_position(reference, *point);
                if let Some(from) = current {
                    add_path_segment(&mut segments, from, to);
                }
                current = Some(to);
            }
            PathCommand::QuadraticTo { control, end } => {
                let Some(from) = current else { continue };
                let control = resolve_path_position(reference, *control);
                let end = resolve_path_position(reference, *end);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let to = [
                        inverse * inverse * from[0]
                            + 2.0 * inverse * t * control[0]
                            + t * t * end[0],
                        inverse * inverse * from[1]
                            + 2.0 * inverse * t * control[1]
                            + t * t * end[1],
                    ];
                    add_path_segment(&mut segments, previous, to);
                    previous = to;
                }
                current = Some(end);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                end,
            } => {
                let Some(from) = current else { continue };
                let control_1 = resolve_path_position(reference, *control_1);
                let control_2 = resolve_path_position(reference, *control_2);
                let end = resolve_path_position(reference, *end);
                let mut previous = from;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let inverse = 1.0 - t;
                    let to = [
                        inverse.powi(3) * from[0]
                            + 3.0 * inverse * inverse * t * control_1[0]
                            + 3.0 * inverse * t * t * control_2[0]
                            + t.powi(3) * end[0],
                        inverse.powi(3) * from[1]
                            + 3.0 * inverse * inverse * t * control_1[1]
                            + 3.0 * inverse * t * t * control_2[1]
                            + t.powi(3) * end[1],
                    ];
                    add_path_segment(&mut segments, previous, to);
                    previous = to;
                }
                current = Some(end);
            }
            PathCommand::Close => close_path_subpath(&mut segments, &mut current, start),
        }
    }
    close_path_subpath(&mut segments, &mut current, start);
    segments
}

pub(super) fn path_bounds(segments: &[PathSegment]) -> Option<LayoutRect> {
    let first = segments.first()?;
    let mut left = first.from[0].min(first.to[0]);
    let mut top = first.from[1].min(first.to[1]);
    let mut right = first.from[0].max(first.to[0]);
    let mut bottom = first.from[1].max(first.to[1]);
    for segment in &segments[1..] {
        left = left.min(segment.from[0]).min(segment.to[0]);
        top = top.min(segment.from[1]).min(segment.to[1]);
        right = right.max(segment.from[0]).max(segment.to[0]);
        bottom = bottom.max(segment.from[1]).max(segment.to[1]);
    }
    Some(LayoutRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

pub(super) fn inset_clip_rect(
    reference: LayoutRect,
    edges: &whisker_protocol::PaintEdges<PaintCoordinate>,
) -> LayoutRect {
    let top = resolve_coordinate(edges.top, reference.height);
    let right = resolve_coordinate(edges.right, reference.width);
    let bottom = resolve_coordinate(edges.bottom, reference.height);
    let left = resolve_coordinate(edges.left, reference.width);
    LayoutRect {
        x: reference.x + left,
        y: reference.y + top,
        width: (reference.width - left - right).max(0.0),
        height: (reference.height - top - bottom).max(0.0),
    }
}

pub(super) fn resolve_coordinate(value: PaintCoordinate, available: f32) -> f32 {
    value.length + value.fraction * available
}

pub(super) fn resolve_length_percentage(
    value: whisker_protocol::PaintLengthPercentage,
    available: f32,
) -> f32 {
    value.length + value.fraction * available
}

pub(crate) fn is_transparent(color: &PaintColor) -> bool {
    matches!(
        color,
        PaintColor::Srgba { alpha, .. } | PaintColor::Hsla { alpha, .. } if *alpha == 0.0
    ) || matches!(color, PaintColor::Named(name) if name.eq_ignore_ascii_case("transparent"))
}
