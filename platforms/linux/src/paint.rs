use gtk::prelude::*;
use gtk::{gdk, graphene, gsk};
use gtk4 as gtk;
use whisker_protocol::{
    BackgroundLayer, BackgroundSize, BorderLineStyle, BoxPaint, BoxShadow, ClipShape, FillRule,
    GradientStop, ImageRepeat, LayoutGeometry, OutlineLineStyle, OutlinePaint, PaintBox,
    PaintCoordinate, PaintImage, PaintLengthPercentage, PaintPosition, PathCommand,
};

use crate::color;

pub(crate) fn resolve(value: PaintLengthPercentage, basis: f32) -> f32 {
    value.length + value.fraction * basis
}

fn coordinate(value: PaintCoordinate, start: f32, extent: f32) -> f32 {
    start + value.length + value.fraction * extent
}

/// Border widths in top, right, bottom, left order; `none` and `hidden` edges are zero.
pub(crate) fn border_widths(paint: &BoxPaint, bounds: &graphene::Rect) -> [f32; 4] {
    [
        (paint.border_widths.top, paint.border_styles.top),
        (paint.border_widths.right, paint.border_styles.right),
        (paint.border_widths.bottom, paint.border_styles.bottom),
        (paint.border_widths.left, paint.border_styles.left),
    ]
    .map(|(width, style)| {
        if matches!(style, BorderLineStyle::None | BorderLineStyle::Hidden) {
            0.0
        } else {
            resolve(width, bounds.width())
        }
    })
}

pub(crate) fn rounded_rect(paint: Option<&BoxPaint>, bounds: &graphene::Rect) -> gsk::RoundedRect {
    let Some(paint) = paint else {
        return gsk::RoundedRect::from_rect(*bounds, 0.0);
    };
    let corner = |radius: whisker_protocol::PaintCornerRadius| {
        let width = resolve(radius.horizontal, bounds.width()).max(0.0);
        let height = resolve(radius.vertical, bounds.height()).max(0.0);
        if width == 0.0 || height == 0.0 {
            graphene::Size::zero()
        } else {
            graphene::Size::new(width, height)
        }
    };
    let radii = &paint.border_radii;
    let mut rect = gsk::RoundedRect::new(
        *bounds,
        corner(radii.top_left),
        corner(radii.top_right),
        corner(radii.bottom_right),
        corner(radii.bottom_left),
    );
    rect.normalize();
    rect
}

/// The border, padding, or content edge of a box with correspondingly reduced corners.
pub(crate) fn paint_box_rect(
    which: PaintBox,
    paint: Option<&BoxPaint>,
    geometry: &LayoutGeometry,
) -> gsk::RoundedRect {
    let border = geometry.border_box;
    let bounds = graphene::Rect::new(0.0, 0.0, border.width, border.height);
    let outer = rounded_rect(paint, &bounds);
    let [top, right, bottom, left] = match which {
        PaintBox::Padding => paint.map_or([0.0; 4], |paint| border_widths(paint, &bounds)),
        PaintBox::Content => {
            let content = geometry.content_box;
            [
                content.y,
                border.width - content.x - content.width,
                border.height - content.y - content.height,
                content.x,
            ]
        }
        _ => return outer,
    };
    let mut rect = outer;
    rect.shrink(top, right, bottom, left);
    rect
}

pub(crate) fn paint_background(
    snapshot: &gtk::Snapshot,
    paint: Option<&BoxPaint>,
    layers: &[BackgroundLayer],
    geometry: &LayoutGeometry,
) {
    if let Some(paint) = paint {
        let background = color::rgba(&paint.background_color);
        if !color::is_transparent(&background) {
            // CSS clips the background color with the bottom layer's clip box.
            let clip = layers.last().map_or(PaintBox::Border, |layer| layer.clip);
            fill(
                snapshot,
                &paint_box_rect(clip, Some(paint), geometry),
                &background,
            );
        }
    }
    for layer in layers.iter().rev() {
        paint_layer(snapshot, layer, paint, geometry);
    }
}

fn fill(snapshot: &gtk::Snapshot, rect: &gsk::RoundedRect, color: &gdk::RGBA) {
    if rect.is_rectilinear() {
        snapshot.append_color(color, rect.bounds());
    } else {
        snapshot.push_rounded_clip(rect);
        snapshot.append_color(color, rect.bounds());
        snapshot.pop();
    }
}

/// The `clip-path` shape resolved against its reference box, with its fill rule.
pub(crate) fn clip_path(
    paint: Option<&BoxPaint>,
    geometry: &LayoutGeometry,
    reference: PaintBox,
    shape: &ClipShape,
) -> (gsk::Path, gsk::FillRule) {
    let area = *paint_box_rect(reference, paint, geometry).bounds();
    let (x, y, width, height) = (area.x(), area.y(), area.width(), area.height());
    let point = |position: PaintPosition| {
        graphene::Point::new(
            coordinate(position.x, x, width),
            coordinate(position.y, y, height),
        )
    };
    let builder = gsk::PathBuilder::new();
    let mut fill_rule = gsk::FillRule::Winding;
    match shape {
        ClipShape::Inset { edges, radii } => {
            let top = coordinate(edges.top, 0.0, height);
            let right = coordinate(edges.right, 0.0, width);
            let bottom = coordinate(edges.bottom, 0.0, height);
            let left = coordinate(edges.left, 0.0, width);
            let rect = graphene::Rect::new(
                x + left,
                y + top,
                (width - left - right).max(0.0),
                (height - top - bottom).max(0.0),
            );
            let corner = |radius: whisker_protocol::PaintCornerRadius| {
                graphene::Size::new(
                    resolve(radius.horizontal, rect.width()),
                    resolve(radius.vertical, rect.height()),
                )
            };
            let mut rounded = gsk::RoundedRect::new(
                rect,
                corner(radii.top_left),
                corner(radii.top_right),
                corner(radii.bottom_right),
                corner(radii.bottom_left),
            );
            rounded.normalize();
            builder.add_rounded_rect(&rounded);
        }
        ClipShape::Circle { radius, center } => {
            let radius = resolve(*radius, width.hypot(height) / 2.0_f32.sqrt());
            builder.add_circle(&point(*center), radius.max(0.0));
        }
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center,
        } => {
            let center = point(*center);
            let (rx, ry) = (resolve(*radius_x, width), resolve(*radius_y, height));
            let corner = graphene::Size::new(rx, ry);
            builder.add_rounded_rect(&gsk::RoundedRect::new(
                graphene::Rect::new(center.x() - rx, center.y() - ry, 2.0 * rx, 2.0 * ry),
                corner,
                corner,
                corner,
                corner,
            ));
        }
        ClipShape::Polygon {
            fill_rule: rule,
            points,
        } => {
            if *rule == FillRule::EvenOdd {
                fill_rule = gsk::FillRule::EvenOdd;
            }
            for (index, position) in points.iter().enumerate() {
                let to = point(*position);
                if index == 0 {
                    builder.move_to(to.x(), to.y());
                } else {
                    builder.line_to(to.x(), to.y());
                }
            }
            builder.close();
        }
        ClipShape::Path {
            fill_rule: rule,
            commands,
        } => {
            if *rule == FillRule::EvenOdd {
                fill_rule = gsk::FillRule::EvenOdd;
            }
            for command in commands {
                match command {
                    PathCommand::MoveTo(to) => {
                        let to = point(*to);
                        builder.move_to(to.x(), to.y());
                    }
                    PathCommand::LineTo(to) => {
                        let to = point(*to);
                        builder.line_to(to.x(), to.y());
                    }
                    PathCommand::QuadraticTo { control, end } => {
                        let (control, end) = (point(*control), point(*end));
                        builder.quad_to(control.x(), control.y(), end.x(), end.y());
                    }
                    PathCommand::CubicTo {
                        control_1,
                        control_2,
                        end,
                    } => {
                        let (first, second, end) =
                            (point(*control_1), point(*control_2), point(*end));
                        builder.cubic_to(
                            first.x(),
                            first.y(),
                            second.x(),
                            second.y(),
                            end.x(),
                            end.y(),
                        );
                    }
                    PathCommand::Close => builder.close(),
                }
            }
        }
    }
    (builder.to_path(), fill_rule)
}

/// The area between the border edge and the padding edge.
fn border_ring(paint: Option<&BoxPaint>, geometry: &LayoutGeometry) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_rounded_rect(&paint_box_rect(PaintBox::Border, paint, geometry));
    builder.add_rounded_rect(&paint_box_rect(PaintBox::Padding, paint, geometry));
    builder.to_path()
}

/// Lightens toward white for positive `amount` and darkens toward black for negative.
fn shade(color: gdk::RGBA, amount: f32) -> gdk::RGBA {
    let channel = |value: f32| {
        if amount < 0.0 {
            value * (1.0 + amount)
        } else {
            value + (1.0 - value) * amount
        }
    };
    gdk::RGBA::new(
        channel(color.red()),
        channel(color.green()),
        channel(color.blue()),
        color.alpha(),
    )
}

/// Solid bands of `style` as `(start depth, end depth, shade)`, with depth 0 at the outer edge.
fn border_bands(style: BorderLineStyle, top_or_left: bool) -> &'static [(f32, f32, f32)] {
    let outer = if top_or_left { -0.35 } else { 0.35 };
    match (style, outer < 0.0) {
        (BorderLineStyle::Solid, _) => &[(0.0, 1.0, 0.0)],
        (BorderLineStyle::Double, _) => &[(0.0, 1.0 / 3.0, 0.0), (2.0 / 3.0, 1.0, 0.0)],
        (BorderLineStyle::Inset, true) | (BorderLineStyle::Outset, false) => &[(0.0, 1.0, -0.35)],
        (BorderLineStyle::Inset, false) | (BorderLineStyle::Outset, true) => &[(0.0, 1.0, 0.35)],
        (BorderLineStyle::Groove, true) | (BorderLineStyle::Ridge, false) => {
            &[(0.0, 0.5, -0.35), (0.5, 1.0, 0.35)]
        }
        (BorderLineStyle::Groove, false) | (BorderLineStyle::Ridge, true) => {
            &[(0.0, 0.5, 0.35), (0.5, 1.0, -0.35)]
        }
        _ => &[],
    }
}

pub(crate) fn paint_border(snapshot: &gtk::Snapshot, paint: &BoxPaint, geometry: &LayoutGeometry) {
    let border = geometry.border_box;
    let bounds = graphene::Rect::new(0.0, 0.0, border.width, border.height);
    let widths = border_widths(paint, &bounds);
    if widths.iter().all(|width| *width <= 0.0) {
        return;
    }
    let colors = [
        &paint.border_colors.top,
        &paint.border_colors.right,
        &paint.border_colors.bottom,
        &paint.border_colors.left,
    ]
    .map(color::rgba);
    let styles = [
        paint.border_styles.top,
        paint.border_styles.right,
        paint.border_styles.bottom,
        paint.border_styles.left,
    ];
    let outer = rounded_rect(Some(paint), &bounds);
    let mut ranges = Vec::<(f32, f32)>::new();
    for (side, style) in styles.iter().enumerate() {
        for (start, end, _) in border_bands(*style, side == 0 || side == 3) {
            if !ranges.contains(&(*start, *end)) {
                ranges.push((*start, *end));
            }
        }
    }
    for (start, end) in ranges {
        let band_colors: [Option<gdk::RGBA>; 4] = std::array::from_fn(|side| {
            border_bands(styles[side], side == 0 || side == 3)
                .iter()
                .find(|band| (band.0, band.1) == (start, end))
                .filter(|_| widths[side] > 0.0)
                .map(|(_, _, amount)| shade(colors[side], *amount))
        });
        let ring = gsk::PathBuilder::new();
        for depth in [start, end] {
            let mut rect = outer;
            rect.shrink(
                widths[0] * depth,
                widths[1] * depth,
                widths[2] * depth,
                widths[3] * depth,
            );
            ring.add_rounded_rect(&rect);
        }
        let ring = ring.to_path();
        if band_colors.iter().all(|color| *color == band_colors[0]) {
            if let Some(color) = &band_colors[0] {
                snapshot.append_fill(&ring, gsk::FillRule::EvenOdd, color);
            }
            continue;
        }
        for (side, color) in band_colors.iter().enumerate() {
            if let Some(color) = color {
                snapshot.push_fill(&side_region(side, &widths, &bounds), gsk::FillRule::Winding);
                snapshot.append_fill(&ring, gsk::FillRule::EvenOdd, color);
                snapshot.pop();
            }
        }
    }
    if styles
        .iter()
        .any(|style| matches!(style, BorderLineStyle::Dashed | BorderLineStyle::Dotted))
    {
        snapshot.push_fill(&border_ring(Some(paint), geometry), gsk::FillRule::EvenOdd);
        for side in 0..4 {
            paint_patterned_side(
                snapshot,
                side,
                styles[side],
                &widths,
                &bounds,
                &colors[side],
            );
        }
        snapshot.pop();
    }
}

/// The trapezoid one side owns, joined to its neighbours along the corner diagonals.
fn side_region(side: usize, widths: &[f32; 4], bounds: &graphene::Rect) -> gsk::Path {
    let (w, h) = (bounds.width(), bounds.height());
    let [top, right, bottom, left] = *widths;
    let corners = [
        [(0.0, 0.0), (w, 0.0), (w - right, top), (left, top)],
        [(w, 0.0), (w, h), (w - right, h - bottom), (w - right, top)],
        [
            (w, h),
            (0.0, h),
            (left, h - bottom),
            (w - right, h - bottom),
        ],
        [(0.0, h), (0.0, 0.0), (left, top), (left, h - bottom)],
    ][side];
    let region = gsk::PathBuilder::new();
    region.move_to(corners[0].0, corners[0].1);
    for (x, y) in &corners[1..] {
        region.line_to(*x, *y);
    }
    region.close();
    region.to_path()
}

/// Paints one dashed or dotted side, phased along the clockwise perimeter from the top-left.
fn paint_patterned_side(
    snapshot: &gtk::Snapshot,
    side: usize,
    style: BorderLineStyle,
    widths: &[f32; 4],
    bounds: &graphene::Rect,
    color: &gdk::RGBA,
) {
    let width = widths[side];
    if width <= 0.0 || !matches!(style, BorderLineStyle::Dashed | BorderLineStyle::Dotted) {
        return;
    }
    let (w, h) = (bounds.width(), bounds.height());
    let (start, length) = [(0.0, w), (w, h), (w + h, w), (2.0 * w + h, h)][side];
    // Maps a perimeter position and a depth from the outer edge to box coordinates.
    let point = |position: f32, depth: f32| {
        let along = position - start;
        match side {
            0 => graphene::Point::new(along, depth),
            1 => graphene::Point::new(w - depth, along),
            2 => graphene::Point::new(w - along, h - depth),
            _ => graphene::Point::new(depth, h - along),
        }
    };
    snapshot.push_fill(&side_region(side, widths, bounds), gsk::FillRule::Winding);
    let shapes = gsk::PathBuilder::new();
    let dotted = style == BorderLineStyle::Dotted;
    let period = width * if dotted { 2.0 } else { 4.0 };
    let mut position = (start / period).floor() * period;
    while position < start + length {
        if dotted {
            shapes.add_circle(&point(position + width, width / 2.0), width / 2.0);
        } else {
            let from = point(position.max(start), 0.0);
            let to = point((position + 3.0 * width).min(start + length), width);
            shapes.add_rect(&graphene::Rect::new(
                from.x().min(to.x()),
                from.y().min(to.y()),
                (from.x() - to.x()).abs(),
                (from.y() - to.y()).abs(),
            ));
        }
        position += period;
    }
    snapshot.append_fill(&shapes.to_path(), gsk::FillRule::Winding, color);
    snapshot.pop();
}

/// Paints shadows in CSS order: outset ones under the box, inset ones over its background.
pub(crate) fn paint_box_shadows(
    snapshot: &gtk::Snapshot,
    shadows: &[BoxShadow],
    paint: Option<&BoxPaint>,
    geometry: &LayoutGeometry,
    inset: bool,
) {
    let border = geometry.border_box;
    let bounds = graphene::Rect::new(0.0, 0.0, border.width, border.height);
    for shadow in shadows.iter().rev().filter(|shadow| shadow.inset == inset) {
        let color = color::rgba(&shadow.color);
        if inset {
            let padding = paint_box_rect(PaintBox::Padding, paint, geometry);
            snapshot.append_inset_shadow(
                &padding,
                &color,
                shadow.offset_x,
                shadow.offset_y,
                shadow.spread_radius,
                shadow.blur_radius,
            );
        } else {
            snapshot.append_outset_shadow(
                &rounded_rect(paint, &bounds),
                &color,
                shadow.offset_x,
                shadow.offset_y,
                shadow.spread_radius,
                shadow.blur_radius,
            );
        }
    }
}

pub(crate) fn paint_outline(
    snapshot: &gtk::Snapshot,
    outline: &OutlinePaint,
    paint: Option<&BoxPaint>,
    bounds: &graphene::Rect,
) {
    if outline.width <= 0.0
        || matches!(
            outline.style,
            OutlineLineStyle::Line(BorderLineStyle::None | BorderLineStyle::Hidden)
        )
    {
        return;
    }
    let grow = -(outline.offset + outline.width);
    let mut rect = rounded_rect(paint, bounds);
    rect.shrink(grow, grow, grow, grow);
    let color = color::rgba(&outline.color);
    snapshot.append_border(&rect, &[outline.width; 4], &[color; 4]);
}

struct AxisTiles {
    origin: f32,
    size: f32,
    stride: f32,
    repeats: bool,
}

fn axis_tiles(
    start: f32,
    area: f32,
    image: f32,
    position: PaintCoordinate,
    repeat: ImageRepeat,
) -> AxisTiles {
    let positioned = |size: f32| start + position.length + position.fraction * (area - size);
    match repeat {
        ImageRepeat::Repeat | ImageRepeat::NoRepeat => AxisTiles {
            origin: positioned(image),
            size: image,
            stride: image,
            repeats: repeat == ImageRepeat::Repeat,
        },
        ImageRepeat::Space => {
            let count = (area / image).floor();
            if count >= 2.0 {
                AxisTiles {
                    origin: start,
                    size: image,
                    stride: (area - image) / (count - 1.0),
                    repeats: true,
                }
            } else {
                AxisTiles {
                    origin: positioned(image),
                    size: image,
                    stride: image,
                    repeats: false,
                }
            }
        }
        ImageRepeat::Round => {
            let size = area / (area / image).round().max(1.0);
            AxisTiles {
                origin: positioned(size),
                size,
                stride: size,
                repeats: true,
            }
        }
    }
}

fn paint_layer(
    snapshot: &gtk::Snapshot,
    layer: &BackgroundLayer,
    paint: Option<&BoxPaint>,
    geometry: &LayoutGeometry,
) {
    if matches!(layer.image, PaintImage::None | PaintImage::Resource(_)) {
        return;
    }
    let area = *paint_box_rect(layer.origin, paint, geometry).bounds();
    let [width, height] = match layer.size {
        BackgroundSize::Explicit { width, height } => [
            width.map_or(area.width(), |value| resolve(value, area.width())),
            height.map_or(area.height(), |value| resolve(value, area.height())),
        ],
        BackgroundSize::Auto | BackgroundSize::Cover | BackgroundSize::Contain => {
            [area.width(), area.height()]
        }
    };
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let x = axis_tiles(
        area.x(),
        area.width(),
        width,
        layer.position.x,
        layer.repeat_x,
    );
    let y = axis_tiles(
        area.y(),
        area.height(),
        height,
        layer.position.y,
        layer.repeat_y,
    );
    let tile = graphene::Rect::new(x.origin, y.origin, x.size, y.size);
    let clip = paint_box_rect(layer.clip, paint, geometry);
    if layer.clip == PaintBox::BorderArea {
        snapshot.push_fill(&border_ring(paint, geometry), gsk::FillRule::EvenOdd);
    } else {
        snapshot.push_rounded_clip(&clip);
    }
    if x.repeats || y.repeats {
        let bounds = clip.bounds();
        let repeat_bounds = graphene::Rect::new(
            if x.repeats { bounds.x() } else { tile.x() },
            if y.repeats { bounds.y() } else { tile.y() },
            if x.repeats {
                bounds.width()
            } else {
                tile.width()
            },
            if y.repeats {
                bounds.height()
            } else {
                tile.height()
            },
        );
        let period = graphene::Rect::new(tile.x(), tile.y(), x.stride, y.stride);
        snapshot.push_repeat(&repeat_bounds, Some(&period));
        paint_image(snapshot, &layer.image, &tile);
        snapshot.pop();
    } else {
        paint_image(snapshot, &layer.image, &tile);
    }
    snapshot.pop();
}

/// Stop offsets along the gradient line after CSS position fix-up.
fn stop_offsets(stops: &[GradientStop], length: f32) -> Vec<f32> {
    let mut offsets = stops
        .iter()
        .map(|stop| {
            stop.position
                .map(|position| position.fraction + position.length / length.max(f32::EPSILON))
        })
        .collect::<Vec<_>>();
    if let Some(first) = offsets.first_mut() {
        first.get_or_insert(0.0);
    }
    if let Some(last) = offsets.last_mut() {
        last.get_or_insert(1.0);
    }
    let mut resolved = Vec::with_capacity(offsets.len());
    let mut index = 0;
    while index < offsets.len() {
        match offsets[index] {
            Some(offset) => {
                let floor = resolved.last().copied().unwrap_or(offset);
                resolved.push(offset.max(floor));
                index += 1;
            }
            None => {
                let start = resolved.last().copied().unwrap_or(0.0);
                let next = (index..offsets.len())
                    .find(|candidate| offsets[*candidate].is_some())
                    .expect("the last stop always has a position");
                let end = offsets[next].unwrap_or(1.0).max(start);
                let gaps = (next - index + 1) as f32;
                for step in 1..=(next - index) {
                    resolved.push(start + (end - start) * step as f32 / gaps);
                }
                index = next;
            }
        }
    }
    resolved
}

/// Normalizes offsets into GSK's `0..=1` range and returns the covered span.
fn color_stops(stops: &[GradientStop], offsets: &[f32]) -> (f32, f32, Vec<gsk::ColorStop>) {
    let first = offsets.first().copied().unwrap_or(0.0);
    let last = offsets.last().copied().unwrap_or(1.0);
    let span = (last - first).max(1.0e-4);
    let stops = stops
        .iter()
        .zip(offsets)
        .map(|(stop, offset)| {
            gsk::ColorStop::new(
                ((offset - first) / span).clamp(0.0, 1.0),
                color::rgba(&stop.color),
            )
        })
        .collect();
    (first, first + span, stops)
}

fn paint_image(snapshot: &gtk::Snapshot, image: &PaintImage, tile: &graphene::Rect) {
    let (w, h) = (tile.width(), tile.height());
    match image {
        PaintImage::LinearGradient {
            angle_degrees,
            repeating,
            stops,
        } => {
            let (sin, cos) = angle_degrees.to_radians().sin_cos();
            let length = (w * sin).abs() + (h * cos).abs();
            let (first, last, color_stops) = color_stops(stops, &stop_offsets(stops, length));
            let center = (tile.x() + w / 2.0, tile.y() + h / 2.0);
            let point = |t: f32| {
                let along = (t - 0.5) * length;
                graphene::Point::new(center.0 + sin * along, center.1 - cos * along)
            };
            if *repeating {
                snapshot.append_repeating_linear_gradient(
                    tile,
                    &point(first),
                    &point(last),
                    &color_stops,
                );
            } else {
                snapshot.append_linear_gradient(tile, &point(first), &point(last), &color_stops);
            }
        }
        PaintImage::RadialGradient {
            center,
            radii,
            repeating,
            stops,
            ..
        } => {
            let Some((rx, ry)) = radii else {
                return;
            };
            let (rx, ry) = (resolve(*rx, w).max(1.0e-3), resolve(*ry, h).max(1.0e-3));
            let center = position(*center, tile);
            let (first, last, color_stops) = color_stops(stops, &stop_offsets(stops, rx));
            let (start, end) = (first.max(0.0), last.max(first.max(0.0) + 1.0e-4));
            if *repeating {
                snapshot.append_repeating_radial_gradient(
                    tile,
                    &center,
                    rx,
                    ry,
                    start,
                    end,
                    &color_stops,
                );
            } else {
                snapshot.append_radial_gradient(tile, &center, rx, ry, start, end, &color_stops);
            }
        }
        PaintImage::ConicGradient {
            from_degrees,
            center,
            stops,
            ..
        } => {
            let offsets = stop_offsets(stops, 360.0);
            let color_stops = stops
                .iter()
                .zip(&offsets)
                .map(|(stop, offset)| {
                    gsk::ColorStop::new(offset.clamp(0.0, 1.0), color::rgba(&stop.color))
                })
                .collect::<Vec<_>>();
            snapshot.append_conic_gradient(
                tile,
                &position(*center, tile),
                *from_degrees,
                &color_stops,
            );
        }
        PaintImage::None | PaintImage::Resource(_) => {}
    }
}

fn position(value: PaintPosition, rect: &graphene::Rect) -> graphene::Point {
    graphene::Point::new(
        coordinate(value.x, rect.x(), rect.width()),
        coordinate(value.y, rect.y(), rect.height()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::PaintColor;

    fn stop(position: Option<f32>) -> GradientStop {
        GradientStop {
            color: PaintColor::default(),
            position: position.map(|fraction| PaintCoordinate {
                length: 0.0,
                fraction,
            }),
        }
    }

    #[test]
    fn missing_stop_positions_are_spread_and_kept_monotonic() {
        let stops = [
            stop(None),
            stop(None),
            stop(Some(0.6)),
            stop(Some(0.4)),
            stop(None),
        ];
        assert_eq!(stop_offsets(&stops, 100.0), [0.0, 0.3, 0.6, 0.6, 1.0]);
    }

    #[test]
    fn space_repetition_distributes_whole_tiles() {
        let tiles = axis_tiles(
            0.0,
            100.0,
            30.0,
            PaintCoordinate::default(),
            ImageRepeat::Space,
        );
        assert_eq!((tiles.origin, tiles.stride), (0.0, 35.0));
        let round = axis_tiles(
            0.0,
            100.0,
            30.0,
            PaintCoordinate::default(),
            ImageRepeat::Round,
        );
        assert_eq!(round.size, 100.0 / 3.0);
    }
}
