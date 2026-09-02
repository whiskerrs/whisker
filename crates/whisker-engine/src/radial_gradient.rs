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

#[cfg(test)]
mod tests {
    use whisker_protocol::{
        BackgroundAttachment, BlendMode, GradientStop, LayoutRect, PaintColor, PaintEdges,
        PaintPosition, ResourceId,
    };

    use super::*;

    fn coordinate(length: f32, fraction: f32) -> PaintCoordinate {
        PaintCoordinate { length, fraction }
    }

    fn length(length: f32, fraction: f32) -> PaintLengthPercentage {
        PaintLengthPercentage { length, fraction }
    }

    fn radial(
        shape: RadialGradientShape,
        extent: RadialGradientExtent,
        radii: Option<(PaintLengthPercentage, PaintLengthPercentage)>,
        stop: PaintCoordinate,
    ) -> BackgroundLayer {
        radial_with_first_stop(shape, extent, radii, Some(coordinate(0.0, 0.0)), stop)
    }

    fn radial_with_first_stop(
        shape: RadialGradientShape,
        extent: RadialGradientExtent,
        radii: Option<(PaintLengthPercentage, PaintLengthPercentage)>,
        first_stop: Option<PaintCoordinate>,
        stop: PaintCoordinate,
    ) -> BackgroundLayer {
        BackgroundLayer {
            image: PaintImage::RadialGradient {
                shape,
                extent,
                center: PaintPosition {
                    x: coordinate(0.0, 0.25),
                    y: coordinate(0.0, 0.75),
                },
                radii,
                repeating: false,
                stops: vec![
                    GradientStop {
                        color: PaintColor::default(),
                        position: first_stop,
                    },
                    GradientStop {
                        color: PaintColor::default(),
                        position: Some(stop),
                    },
                ],
            },
            position: PaintPosition::default(),
            size: BackgroundSize::Auto,
            repeat_x: ImageRepeat::Repeat,
            repeat_y: ImageRepeat::Repeat,
            origin: PaintBox::Border,
            clip: PaintBox::Border,
            attachment: BackgroundAttachment::Scroll,
            blend_mode: BlendMode::Normal,
        }
    }

    fn resource() -> BackgroundLayer {
        let mut layer = radial(
            RadialGradientShape::Circle,
            RadialGradientExtent::ClosestSide,
            None,
            coordinate(0.0, 1.0),
        );
        layer.image = PaintImage::Resource(ResourceId::new(1).expect("resource"));
        layer
    }

    fn source(shape: RadialGradientShape, extent: RadialGradientExtent) -> RadialBackgroundSource {
        RadialBackgroundSource {
            layer: 0,
            shape,
            extent,
            radii: None,
            original_stop_positions: None,
        }
    }

    #[test]
    fn source_discovery_skips_non_radial_and_already_canonical_layers() {
        let absolute = coordinate(0.0, 0.5);
        let layers = vec![
            resource(),
            radial(
                RadialGradientShape::Circle,
                RadialGradientExtent::Explicit,
                Some((absolute_length(1.0), absolute_length(1.0))),
                absolute,
            ),
            radial(
                RadialGradientShape::Ellipse,
                RadialGradientExtent::ClosestSide,
                Some((absolute_length(1.0), absolute_length(1.0))),
                absolute,
            ),
            radial(
                RadialGradientShape::Ellipse,
                RadialGradientExtent::Explicit,
                None,
                absolute,
            ),
            radial(
                RadialGradientShape::Ellipse,
                RadialGradientExtent::Explicit,
                Some((absolute_length(1.0), absolute_length(1.0))),
                absolute,
            ),
            radial(
                RadialGradientShape::Ellipse,
                RadialGradientExtent::Explicit,
                Some((absolute_length(1.0), absolute_length(1.0))),
                coordinate(4.0, 0.5),
            ),
        ];
        let found = sources(&layers);
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].layer, 1);
        assert_eq!(found[3].layer, 5);
        assert!(found[3].original_stop_positions.is_some());
    }

    #[test]
    fn canonicalization_tolerates_stale_sources_and_unresolved_explicit_radii() {
        let geometry = LayoutGeometry::from(LayoutRect {
            width: 100.0,
            height: 80.0,
            ..LayoutRect::default()
        });
        let stale = RadialBackgroundSource {
            layer: 1,
            ..source(
                RadialGradientShape::Circle,
                RadialGradientExtent::ClosestSide,
            )
        };
        let mut no_layer = vec![resource()];
        canonicalize(&mut no_layer, &[stale], geometry, None);

        let wrong_kind = source(
            RadialGradientShape::Circle,
            RadialGradientExtent::ClosestSide,
        );
        canonicalize(&mut no_layer, &[wrong_kind], geometry, None);

        let missing_radii = source(RadialGradientShape::Ellipse, RadialGradientExtent::Explicit);
        let mut layers = vec![radial(
            RadialGradientShape::Ellipse,
            RadialGradientExtent::Explicit,
            None,
            coordinate(0.0, 0.5),
        )];
        let unchanged = layers.clone();
        canonicalize(&mut layers, &[missing_radii], geometry, None);
        assert_eq!(layers, unchanged);

        let mut resolved = vec![radial_with_first_stop(
            RadialGradientShape::Circle,
            RadialGradientExtent::ClosestSide,
            None,
            None,
            coordinate(0.0, 0.5),
        )];
        canonicalize(
            &mut resolved,
            &[source(
                RadialGradientShape::Circle,
                RadialGradientExtent::ClosestSide,
            )],
            geometry,
            None,
        );
        let expected = radial_with_first_stop(
            RadialGradientShape::Ellipse,
            RadialGradientExtent::Explicit,
            Some((absolute_length(20.0), absolute_length(20.0))),
            None,
            coordinate(0.0, 0.5),
        );
        assert_eq!(resolved[0].image, expected.image);
    }

    #[test]
    fn positioning_and_tile_geometry_cover_every_supported_mode() {
        let geometry = LayoutGeometry {
            border_box: LayoutRect {
                width: 100.0,
                height: 80.0,
                ..LayoutRect::default()
            },
            content_box: LayoutRect {
                width: 70.0,
                height: 50.0,
                ..LayoutRect::default()
            },
        };
        assert_eq!(
            background_positioning_size(geometry, None, PaintBox::Content),
            [70.0, 50.0]
        );
        assert_eq!(
            background_positioning_size(geometry, None, PaintBox::Padding),
            [100.0, 80.0]
        );
        assert_eq!(
            background_positioning_size(geometry, None, PaintBox::Margin),
            [100.0, 80.0]
        );

        let paint = BoxPaint {
            border_widths: PaintEdges {
                top: length(2.0, 0.1),
                right: length(3.0, 0.1),
                bottom: length(4.0, 0.1),
                left: length(5.0, 0.1),
            },
            ..BoxPaint::default()
        };
        assert_eq!(
            background_positioning_size(geometry, Some(&paint), PaintBox::Padding),
            [72.0, 58.0]
        );

        let mut layer = resource();
        for size in [
            BackgroundSize::Auto,
            BackgroundSize::Cover,
            BackgroundSize::Contain,
        ] {
            layer.size = size;
            assert_eq!(gradient_tile_size(100.0, 80.0, &layer), [100.0, 80.0]);
        }
        layer.size = BackgroundSize::Explicit {
            width: Some(length(10.0, 0.5)),
            height: None,
        };
        layer.repeat_x = ImageRepeat::Round;
        layer.repeat_y = ImageRepeat::Round;
        assert_eq!(gradient_tile_size(100.0, 80.0, &layer), [50.0, 80.0]);
        layer.size = BackgroundSize::Explicit {
            width: None,
            height: Some(length(10.0, 0.5)),
        };
        assert_eq!(gradient_tile_size(-1.0, -2.0, &layer), [0.0, 9.0]);
        assert_eq!(rounded_tile_size(0.0, 2.0), 2.0);
        assert_eq!(rounded_tile_size(2.0, -1.0), 0.0);
        assert!((rounded_tile_size(100.0, 40.0) - 100.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn radius_resolution_covers_all_circle_and_ellipse_extents() {
        let expected = [
            (RadialGradientExtent::ClosestSide, (20.0, 20.0)),
            (RadialGradientExtent::FarthestSide, (75.0, 75.0)),
            (
                RadialGradientExtent::ClosestCorner,
                (25.0_f32.hypot(20.0), 25.0_f32.hypot(20.0)),
            ),
            (
                RadialGradientExtent::FarthestCorner,
                (75.0_f32.hypot(60.0), 75.0_f32.hypot(60.0)),
            ),
        ];
        for (extent, circle_expected) in expected {
            assert_eq!(
                resolved_radii(
                    &source(RadialGradientShape::Circle, extent),
                    100.0,
                    80.0,
                    25.0,
                    20.0,
                ),
                Some(circle_expected)
            );
            assert!(
                resolved_radii(
                    &source(RadialGradientShape::Ellipse, extent),
                    100.0,
                    80.0,
                    25.0,
                    20.0,
                )
                .is_some()
            );
        }

        let explicit_ellipse = RadialBackgroundSource {
            radii: Some((length(10.0, 0.5), length(4.0, 0.25))),
            ..source(RadialGradientShape::Ellipse, RadialGradientExtent::Explicit)
        };
        assert_eq!(
            resolved_radii(&explicit_ellipse, 100.0, 80.0, 25.0, 20.0),
            Some((60.0, 24.0))
        );
        let explicit_circle = RadialBackgroundSource {
            radii: explicit_ellipse.radii,
            ..source(RadialGradientShape::Circle, RadialGradientExtent::Explicit)
        };
        let circle = resolved_radii(&explicit_circle, 100.0, 80.0, 25.0, 20.0).unwrap();
        assert_eq!(circle.0, circle.1);

        let invalid = RadialBackgroundSource {
            radii: Some((absolute_length(f32::NAN), absolute_length(1.0))),
            ..explicit_ellipse
        };
        assert_eq!(resolved_radii(&invalid, 100.0, 80.0, 25.0, 20.0), None);
        assert_eq!(ellipse_corner_scale(0.0, 0.0, 4.0, 3.0), 0.0);
    }
}
