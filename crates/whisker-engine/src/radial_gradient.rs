//! Layout-dependent canonicalization for the radial-gradient wire subset.

use std::sync::Arc;

use whisker_protocol::{
    BackgroundLayer, BackgroundSize, BoxPaint, ImageRepeat, LayoutGeometry, PaintBox,
    PaintCoordinate, PaintImage, PaintLengthPercentage, RadialGradientExtent, RadialGradientShape,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RadialBackgroundSource {
    layer: usize,
    shape: RadialGradientShape,
    extent: RadialGradientExtent,
    radii: Option<(PaintLengthPercentage, PaintLengthPercentage)>,
    original_stop_positions: Option<Arc<[Option<PaintCoordinate>]>>,
}

pub(super) fn sources(layers: &[BackgroundLayer]) -> Vec<RadialBackgroundSource> {
    layers
        .iter()
        .enumerate()
        .filter_map(|(layer, background)| {
            let PaintImage::RadialGradient {
                shape,
                extent,
                radii,
                stops,
                ..
            } = &background.image
            else {
                return None;
            };
            let already_canonical = *shape == RadialGradientShape::Ellipse
                && *extent == RadialGradientExtent::Explicit
                && radii.is_some()
                && stops
                    .iter()
                    .all(|stop| stop.position.is_some_and(|position| position.length == 0.0));
            (!already_canonical).then_some(RadialBackgroundSource {
                layer,
                shape: *shape,
                extent: *extent,
                radii: *radii,
                original_stop_positions: stops
                    .iter()
                    .any(|stop| stop.position.is_some_and(|position| position.length != 0.0))
                    .then(|| stops.iter().map(|stop| stop.position).collect::<Arc<[_]>>()),
            })
        })
        .collect()
}

pub(super) fn canonicalize(
    layers: &mut [BackgroundLayer],
    sources: &[RadialBackgroundSource],
    geometry: LayoutGeometry,
    paint: Option<&BoxPaint>,
) {
    for source in sources {
        let Some(layer) = layers.get_mut(source.layer) else {
            continue;
        };
        let [positioning_width, positioning_height] =
            background_positioning_size(geometry, paint, layer.origin);
        let [tile_width, tile_height] =
            gradient_tile_size(positioning_width, positioning_height, layer);
        let PaintImage::RadialGradient {
            shape,
            extent,
            center,
            radii,
            stops,
            ..
        } = &mut layer.image
        else {
            continue;
        };
        let center_x = center.x.length + center.x.fraction * tile_width;
        let center_y = center.y.length + center.y.fraction * tile_height;
        let Some((radius_x, radius_y)) =
            resolved_radii(source, tile_width, tile_height, center_x, center_y)
        else {
            continue;
        };
        if let Some(originals) = &source.original_stop_positions {
            for (stop, original) in stops.iter_mut().zip(originals.iter()) {
                stop.position = *original;
            }
        }
        let gradient_line = radius_x.max(f32::EPSILON);
        for stop in stops {
            if let Some(position) = &mut stop.position {
                position.fraction += position.length / gradient_line;
                position.length = 0.0;
            }
        }
        *shape = RadialGradientShape::Ellipse;
        *extent = RadialGradientExtent::Explicit;
        *radii = Some((absolute_length(radius_x), absolute_length(radius_y)));
    }
}

fn background_positioning_size(
    geometry: LayoutGeometry,
    paint: Option<&BoxPaint>,
    origin: PaintBox,
) -> [f32; 2] {
    let border = geometry.border_box;
    match origin {
        PaintBox::Content => [geometry.content_box.width, geometry.content_box.height],
        PaintBox::Padding => {
            let Some(paint) = paint else {
                return [border.width, border.height];
            };
            let horizontal = resolve_length(paint.border_widths.left, border.width)
                + resolve_length(paint.border_widths.right, border.width);
            let vertical = resolve_length(paint.border_widths.top, border.height)
                + resolve_length(paint.border_widths.bottom, border.height);
            [
                (border.width - horizontal).max(0.0),
                (border.height - vertical).max(0.0),
            ]
        }
        _ => [border.width, border.height],
    }
}

fn gradient_tile_size(width: f32, height: f32, layer: &BackgroundLayer) -> [f32; 2] {
    let [mut tile_width, mut tile_height] = match layer.size {
        BackgroundSize::Auto | BackgroundSize::Cover | BackgroundSize::Contain => [width, height],
        BackgroundSize::Explicit {
            width: explicit_width,
            height: explicit_height,
        } => [
            explicit_width.map_or(width, |value| resolve_length(value, width)),
            explicit_height.map_or(height, |value| resolve_length(value, height)),
        ],
    };
    if layer.repeat_x == ImageRepeat::Round {
        tile_width = rounded_tile_size(width, tile_width);
    }
    if layer.repeat_y == ImageRepeat::Round {
        tile_height = rounded_tile_size(height, tile_height);
    }
    [tile_width.max(0.0), tile_height.max(0.0)]
}

fn rounded_tile_size(area: f32, tile: f32) -> f32 {
    if area <= 0.0 || tile <= 0.0 {
        return tile.max(0.0);
    }
    let count = (area / tile).round().max(1.0);
    area / count
}

fn resolved_radii(
    source: &RadialBackgroundSource,
    width: f32,
    height: f32,
    center_x: f32,
    center_y: f32,
) -> Option<(f32, f32)> {
    let left = center_x.abs();
    let right = (width - center_x).abs();
    let top = center_y.abs();
    let bottom = (height - center_y).abs();
    let closest_x = left.min(right);
    let farthest_x = left.max(right);
    let closest_y = top.min(bottom);
    let farthest_y = top.max(bottom);
    let circle = source.shape == RadialGradientShape::Circle;
    let radii = match source.extent {
        RadialGradientExtent::Explicit => {
            let (radius_x, radius_y) = source.radii?;
            if circle {
                let basis = width.hypot(height) / 2.0_f32.sqrt();
                let radius = resolve_length(radius_x, basis);
                (radius, radius)
            } else {
                (
                    resolve_length(radius_x, width),
                    resolve_length(radius_y, height),
                )
            }
        }
        RadialGradientExtent::ClosestSide if circle => {
            let radius = closest_x.min(closest_y);
            (radius, radius)
        }
        RadialGradientExtent::FarthestSide if circle => {
            let radius = farthest_x.max(farthest_y);
            (radius, radius)
        }
        RadialGradientExtent::ClosestCorner if circle => {
            let radius = closest_x.hypot(closest_y);
            (radius, radius)
        }
        RadialGradientExtent::FarthestCorner if circle => {
            let radius = farthest_x.hypot(farthest_y);
            (radius, radius)
        }
        RadialGradientExtent::ClosestSide => (closest_x, closest_y),
        RadialGradientExtent::FarthestSide => (farthest_x, farthest_y),
        RadialGradientExtent::ClosestCorner => {
            let scale = ellipse_corner_scale(closest_x, closest_y, closest_x, closest_y);
            (closest_x * scale, closest_y * scale)
        }
        RadialGradientExtent::FarthestCorner => {
            let scale = ellipse_corner_scale(farthest_x, farthest_y, farthest_x, farthest_y);
            (farthest_x * scale, farthest_y * scale)
        }
    };
    (radii.0.is_finite() && radii.1.is_finite()).then_some(radii)
}

fn ellipse_corner_scale(radius_x: f32, radius_y: f32, x: f32, y: f32) -> f32 {
    let normalized_x = if radius_x > 0.0 { x / radius_x } else { 0.0 };
    let normalized_y = if radius_y > 0.0 { y / radius_y } else { 0.0 };
    normalized_x.hypot(normalized_y)
}

fn resolve_length(value: PaintLengthPercentage, extent: f32) -> f32 {
    value.length + value.fraction * extent
}

pub(super) const fn absolute_length(length: f32) -> PaintLengthPercentage {
    PaintLengthPercentage {
        length,
        fraction: 0.0,
    }
}
