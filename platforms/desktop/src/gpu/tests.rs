use super::*;
use glyphon::Color as TextColor;
use whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, BorderLineStyle, BoxPaint,
    ImageRepeat, LayoutRect, PaintBox, PaintColor, PaintCoordinate, PaintCornerRadius,
    PaintCorners, PaintEdges, PaintImage, PaintLengthPercentage, PaintPosition,
    RadialGradientExtent, RadialGradientShape, ResourceId,
};

use crate::paint::box_paint::{ResolvedRadii, resolve_box_geometry, resolve_radii};
use crate::paint::color::srgba;

fn radius(length: f32, fraction: f32) -> PaintCornerRadius {
    PaintCornerRadius::circular(PaintLengthPercentage { length, fraction })
}

fn lower_vertices(rect: LayoutRect, paint: &BoxPaint, opacity: f32, vertices: &mut Vec<BoxVertex>) {
    lower_box(rect, paint, opacity, |primitive| {
        push_quad(vertices, primitive);
    });
}

fn paint(background_color: PaintColor) -> BoxPaint {
    let zero = PaintLengthPercentage::default();
    BoxPaint {
        background_color,
        border_widths: PaintEdges {
            top: PaintLengthPercentage {
                length: 1.0,
                fraction: 0.0,
            },
            right: zero,
            bottom: zero,
            left: zero,
        },
        border_colors: PaintEdges {
            top: PaintColor::Named("blue".into()),
            right: PaintColor::default(),
            bottom: PaintColor::default(),
            left: PaintColor::default(),
        },
        border_styles: PaintEdges {
            top: BorderLineStyle::Solid,
            right: BorderLineStyle::None,
            bottom: BorderLineStyle::Hidden,
            left: BorderLineStyle::None,
        },
        border_radii: PaintCorners {
            top_left: PaintCornerRadius::default(),
            top_right: PaintCornerRadius::default(),
            bottom_right: PaintCornerRadius::default(),
            bottom_left: PaintCornerRadius::default(),
        },
    }
}

#[test]
fn radial_gradient_keyword_extents_resolve_against_the_image_box() {
    let bounds = LayoutRect {
        x: 10.0,
        y: 20.0,
        width: 200.0,
        height: 100.0,
    };
    let center = PaintPosition {
        x: PaintCoordinate {
            length: 0.0,
            fraction: 0.5,
        },
        y: PaintCoordinate {
            length: 0.0,
            fraction: 0.5,
        },
    };

    let circle = radial_gradient_radii(
        bounds,
        center,
        RadialGradientShape::Circle,
        RadialGradientExtent::FarthestCorner,
        None,
    );
    assert!((circle[0] - 111.8034).abs() < 0.001);
    assert_eq!(circle[0], circle[1]);

    let ellipse = radial_gradient_radii(
        bounds,
        center,
        RadialGradientShape::Ellipse,
        RadialGradientExtent::FarthestCorner,
        None,
    );
    assert!((ellipse[0] - 141.42136).abs() < 0.001);
    assert!((ellipse[1] - 70.71068).abs() < 0.001);
}

#[test]
fn explicit_circle_uses_one_radius_on_both_axes() {
    let radius = PaintLengthPercentage {
        length: 40.0,
        fraction: 0.0,
    };
    assert_eq!(
        radial_gradient_radii(
            LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
            PaintPosition::default(),
            RadialGradientShape::Circle,
            RadialGradientExtent::Explicit,
            Some((radius, radius)),
        ),
        [40.0, 40.0]
    );
}

fn resource_layer(size: BackgroundSize) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::Resource(ResourceId::new(1).unwrap()),
        position: PaintPosition {
            x: PaintCoordinate {
                length: 0.0,
                fraction: 0.5,
            },
            y: PaintCoordinate {
                length: 0.0,
                fraction: 0.5,
            },
        },
        size,
        repeat_x: ImageRepeat::NoRepeat,
        repeat_y: ImageRepeat::NoRepeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

#[test]
fn intrinsic_background_sizes_preserve_resource_aspect_ratio() {
    let area = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 80.0,
    };
    let intrinsic = [4.0, 2.0];

    let auto =
        background_tile_geometry(area, &resource_layer(BackgroundSize::Auto), Some(intrinsic))
            .unwrap();
    assert_eq!(
        auto.rect,
        LayoutRect {
            x: 48.0,
            y: 39.0,
            width: 4.0,
            height: 2.0
        }
    );

    let contain = background_tile_geometry(
        area,
        &resource_layer(BackgroundSize::Contain),
        Some(intrinsic),
    )
    .unwrap();
    assert_eq!(
        contain.rect,
        LayoutRect {
            x: 0.0,
            y: 15.0,
            width: 100.0,
            height: 50.0
        }
    );

    let cover = background_tile_geometry(
        area,
        &resource_layer(BackgroundSize::Cover),
        Some(intrinsic),
    )
    .unwrap();
    assert_eq!(
        cover.rect,
        LayoutRect {
            x: -30.0,
            y: 0.0,
            width: 160.0,
            height: 80.0
        }
    );

    let width = background_tile_geometry(
        area,
        &resource_layer(BackgroundSize::Explicit {
            width: Some(PaintLengthPercentage {
                length: 60.0,
                fraction: 0.0,
            }),
            height: None,
        }),
        Some(intrinsic),
    )
    .unwrap();
    assert_eq!(
        width.rect,
        LayoutRect {
            x: 20.0,
            y: 25.0,
            width: 60.0,
            height: 30.0
        }
    );
}

#[test]
fn one_axis_round_rescales_the_opposite_auto_axis() {
    let area = LayoutRect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 80.0,
    };
    let intrinsic = [4.0, 2.0];

    let mut round_width = resource_layer(BackgroundSize::Explicit {
        width: Some(PaintLengthPercentage {
            length: 40.0,
            fraction: 0.0,
        }),
        height: None,
    });
    round_width.position = PaintPosition::default();
    round_width.repeat_x = ImageRepeat::Round;
    let horizontal = background_tile_geometry(area, &round_width, Some(intrinsic)).unwrap();
    assert!((horizontal.rect.width - 100.0 / 3.0).abs() < 0.001);
    assert!((horizontal.rect.height - 50.0 / 3.0).abs() < 0.001);

    let mut round_height = resource_layer(BackgroundSize::Explicit {
        width: None,
        height: Some(PaintLengthPercentage {
            length: 30.0,
            fraction: 0.0,
        }),
    });
    round_height.position = PaintPosition::default();
    round_height.repeat_y = ImageRepeat::Round;
    let vertical = background_tile_geometry(area, &round_height, Some(intrinsic)).unwrap();
    assert!((vertical.rect.width - 160.0 / 3.0).abs() < 0.001);
    assert!((vertical.rect.height - 80.0 / 3.0).abs() < 0.001);
}

#[test]
fn box_lowering_emits_background_and_visible_borders() {
    let mut vertices = Vec::new();
    lower_vertices(
        LayoutRect {
            x: 2.0,
            y: 3.0,
            width: 20.0,
            height: 10.0,
        },
        &paint(PaintColor::Named("red".into())),
        0.5,
        &mut vertices,
    );
    assert_eq!(vertices.len(), 12);
    assert_eq!(vertices[0].position, [2.0, 3.0]);
    assert_eq!(vertices[0].outer_rect, [2.0, 3.0, 20.0, 10.0]);
    assert_eq!(vertices[0].outer_radii_x, [0.0; 4]);
    assert_eq!(vertices[0].outer_radii_y, [0.0; 4]);
    assert_eq!(vertices[0].mode, -1.0);
    assert!((vertices[0].color[3] - 0.5).abs() < f32::EPSILON);
    assert_eq!(vertices[6].position, [2.0, 3.0]);
    assert_eq!(vertices[8].position, [2.0, 13.0]);
    assert_eq!(vertices[6].inner_rect, [2.0, 4.0, 20.0, 9.0]);
    assert_eq!(vertices[6].border_widths, [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(vertices[6].mode, 1.0);
    assert!(vertices[6].border_colors[0][2] > 0.99);
    assert_eq!(vertices[6].border_colors[1], [0.0; 4]);

    vertices.clear();
    let mut transparent = paint(PaintColor::Named("transparent".into()));
    transparent.border_styles.top = BorderLineStyle::None;
    lower_vertices(LayoutRect::default(), &transparent, 1.0, &mut vertices);
    assert!(vertices.is_empty());
}

#[test]
fn rounded_radii_resolve_percentages_and_scale_overlaps() {
    let resolved = resolve_radii(
        &PaintCorners {
            top_left: radius(30.0, 0.0),
            top_right: radius(0.0, 0.5),
            bottom_right: radius(30.0, 0.0),
            bottom_left: radius(30.0, 0.0),
        },
        LayoutRect {
            x: 5.0,
            y: 6.0,
            width: 100.0,
            height: 40.0,
        },
    );
    assert!((resolved.horizontal[0] - 20.0).abs() < f32::EPSILON);
    assert!((resolved.horizontal[1] - 100.0 / 3.0).abs() < 0.001);
    assert!((resolved.horizontal[2] - 20.0).abs() < f32::EPSILON);
    assert!((resolved.horizontal[3] - 20.0).abs() < f32::EPSILON);
    assert!((resolved.vertical[0] - 20.0).abs() < f32::EPSILON);
    assert!((resolved.vertical[1] - 40.0 / 3.0).abs() < 0.001);
    assert!((resolved.vertical[2] - 20.0).abs() < f32::EPSILON);
    assert!((resolved.vertical[3] - 20.0).abs() < f32::EPSILON);

    let mut rounded = paint(PaintColor::Named("red".into()));
    rounded.border_radii = PaintCorners {
        top_left: radius(8.0, 0.0),
        top_right: PaintCornerRadius::default(),
        bottom_right: PaintCornerRadius::default(),
        bottom_left: PaintCornerRadius::default(),
    };
    let mut vertices = Vec::new();
    lower_vertices(
        LayoutRect {
            x: 1.0,
            y: 2.0,
            width: 20.0,
            height: 10.0,
        },
        &rounded,
        1.0,
        &mut vertices,
    );
    assert_eq!(vertices[0].outer_radii_x, [8.0, 0.0, 0.0, 0.0]);
    assert_eq!(vertices[0].outer_radii_y, [8.0, 0.0, 0.0, 0.0]);
    assert_eq!(vertices[6].inner_radii_x, [8.0, 0.0, 0.0, 0.0]);
    assert_eq!(vertices[6].inner_radii_y, [7.0, 0.0, 0.0, 0.0]);
    assert_eq!(
        resolve_radii(&rounded.border_radii, LayoutRect::default()),
        ResolvedRadii {
            horizontal: [0.0; 4],
            vertical: [0.0; 4],
        }
    );
}

#[test]
fn rounded_border_geometry_preserves_corner_arc_centers() {
    let three = PaintLengthPercentage {
        length: 3.0,
        fraction: 0.0,
    };
    let mut bordered = paint(PaintColor::Named("green".into()));
    bordered.border_widths = PaintEdges {
        top: three,
        right: three,
        bottom: three,
        left: three,
    };
    bordered.border_styles = PaintEdges {
        top: BorderLineStyle::Solid,
        right: BorderLineStyle::Solid,
        bottom: BorderLineStyle::Solid,
        left: BorderLineStyle::Solid,
    };
    bordered.border_colors = PaintEdges {
        top: PaintColor::Named("yellow".into()),
        right: PaintColor::Named("yellow".into()),
        bottom: PaintColor::Named("yellow".into()),
        left: PaintColor::Named("yellow".into()),
    };
    bordered.border_radii = PaintCorners {
        top_left: radius(40.0, 0.0),
        top_right: radius(8.0, 0.0),
        bottom_right: radius(40.0, 0.0),
        bottom_left: radius(8.0, 0.0),
    };
    let geometry = resolve_box_geometry(
        LayoutRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 88.0,
        },
        &bordered,
    );

    assert_eq!(geometry.inner_rect.x, 13.0);
    assert_eq!(geometry.inner_rect.y, 23.0);
    assert_eq!(geometry.inner_rect.width, 194.0);
    assert_eq!(geometry.inner_rect.height, 82.0);
    assert_eq!(geometry.outer_radii.horizontal, [40.0, 8.0, 40.0, 8.0]);
    assert_eq!(geometry.outer_radii.vertical, [40.0, 8.0, 40.0, 8.0]);
    assert_eq!(geometry.inner_radii.horizontal, [37.0, 5.0, 37.0, 5.0]);
    assert_eq!(geometry.inner_radii.vertical, [37.0, 5.0, 37.0, 5.0]);
    assert_eq!(
        geometry.outer_rect.x + geometry.outer_radii.horizontal[0],
        geometry.inner_rect.x + geometry.inner_radii.horizontal[0]
    );
    assert_eq!(
        geometry.outer_rect.y + geometry.outer_radii.vertical[0],
        geometry.inner_rect.y + geometry.inner_radii.vertical[0]
    );

    let mut vertices = Vec::new();
    lower_vertices(geometry.outer_rect, &bordered, 1.0, &mut vertices);
    assert_eq!(vertices.len(), 12);
    assert_eq!(vertices[6].mode, 1.0);
    assert!(
        vertices[6]
            .border_colors
            .windows(2)
            .all(|colors| colors[0] == colors[1])
    );
}

#[test]
fn protocol_colors_convert_to_gpu_and_glyph_colors() {
    let hsl = PaintColor::Hsla {
        hue_degrees: 120.0,
        saturation: 100.0,
        lightness: 50.0,
        alpha: 0.8,
    };
    let [red, green, blue, alpha] = srgba(&hsl, 0.5);
    assert!(red.abs() < f32::EPSILON);
    assert!((green - 1.0).abs() < f32::EPSILON);
    assert!(blue.abs() < f32::EPSILON);
    assert!((alpha - 0.4).abs() < f32::EPSILON);
    assert_eq!(text_color(&hsl, 0.5), TextColor::rgba(0, 255, 0, 102));

    assert_eq!(
        srgba(&PaintColor::Named("not-a-css-color".into()), 1.0),
        [0.0; 4]
    );
}

#[test]
fn text_bounds_scale_and_clamp_logical_clips() {
    let bounds = text_bounds(
        LogicalClip {
            left: Some(-2.0),
            top: Some(3.0),
            right: Some(70.0),
            bottom: None,
        },
        100,
        80,
        2.0,
    );
    assert_eq!(
        bounds,
        TextBounds {
            left: 0,
            top: 6,
            right: 100,
            bottom: 80
        }
    );
}

#[test]
fn text_decoration_styles_lower_to_bounded_line_segments() {
    use whisker_protocol::TextDecorationStyle as Style;

    assert_eq!(
        text_decoration_rects(2.0, 20.0, 8.0, 2.0, Style::Solid).len(),
        1
    );
    assert_eq!(
        text_decoration_rects(2.0, 20.0, 8.0, 2.0, Style::Double).len(),
        2
    );
    for style in [Style::Dotted, Style::Dashed, Style::Wavy] {
        let segments = text_decoration_rects(2.0, 20.0, 8.0, 2.0, style);
        assert!(!segments.is_empty());
        assert!(segments.iter().all(|segment| {
            segment.x >= 2.0
                && segment.x + segment.width <= 22.0
                && segment.width >= 0.0
                && segment.height == 2.0
        }));
    }
}
