use super::*;
use whisker_style::{
    ComputedBoxShadow, ComputedClipPath, ComputedClipPathCommand, ComputedClipPoint,
    ComputedClipShape, ComputedCornerRadius, Corners, Edges, MotionPathPointValue, StyleNumber,
};

fn color(name: &str) -> ColorValue {
    ColorValue::Named(name.into())
}

fn paint_style() -> ComputedPaintStyle {
    ComputedPaintStyle {
        image_rendering: ImageRenderingValue::Pixelated,
        background_color: color("background"),
        background_images: Vec::new(),
        background_layers: vec![Default::default()],
        box_shadows: Vec::new(),
        clip_path: None,
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
    assert_eq!(
        lowered.visual_effects.image_rendering,
        ImageRendering::Pixelated
    );
    for (style_value, protocol_value) in [
        (ImageRenderingValue::Auto, ImageRendering::Auto),
        (ImageRenderingValue::CrispEdges, ImageRendering::CrispEdges),
    ] {
        let mut style = paint_style();
        style.image_rendering = style_value;
        assert_eq!(
            lower_paint(&style, &layout).visual_effects.image_rendering,
            protocol_value
        );
    }
}

#[test]
fn lowers_box_shadows_and_every_clip_path_variant() {
    let layout = ComputedLayoutStyle::default();
    let point = |x_length, x_fraction, y_length, y_fraction| ComputedClipPoint {
        x: ComputedLengthPercentage::new(x_length, x_fraction),
        y: ComputedLengthPercentage::new(y_length, y_fraction),
    };
    let center = point(1.0, 0.25, 2.0, 0.75);

    let mut style = paint_style();
    style.box_shadows = vec![ComputedBoxShadow {
        offset_x: StyleNumber::new(1.0),
        offset_y: StyleNumber::new(-2.0),
        blur_radius: StyleNumber::new(3.0),
        spread_radius: StyleNumber::new(-4.0),
        color: ColorValue::Rgba {
            red: 10,
            green: 20,
            blue: 30,
            alpha: StyleNumber::new(0.4),
        },
        inset: true,
    }];
    let shadows = lower_paint(&style, &layout).visual_effects.box_shadows;
    assert_eq!(
        shadows,
        vec![BoxShadow {
            offset_x: 1.0,
            offset_y: -2.0,
            blur_radius: 3.0,
            spread_radius: -4.0,
            color: PaintColor::Srgba {
                red: 10,
                green: 20,
                blue: 30,
                alpha: 0.4,
            },
            inset: true,
        }]
    );

    for (reference_box, expected) in [
        (ClipBoxValue::Border, PaintBox::Border),
        (ClipBoxValue::Padding, PaintBox::Padding),
        (ClipBoxValue::Content, PaintBox::Content),
        (ClipBoxValue::Fill, PaintBox::Fill),
        (ClipBoxValue::Stroke, PaintBox::Stroke),
        (ClipBoxValue::View, PaintBox::View),
    ] {
        style.clip_path = Some(ComputedClipPath {
            reference_box,
            shape: ComputedClipShape::Circle {
                radius: ComputedLengthPercentage::new(5.0, 0.5),
                center,
            },
        });
        assert_eq!(
            lower_paint(&style, &layout).visual_effects.clip_path,
            Some((
                expected,
                ClipShape::Circle {
                    radius: PaintLengthPercentage {
                        length: 5.0,
                        fraction: 0.5,
                    },
                    center: PaintPosition {
                        x: PaintCoordinate {
                            length: 1.0,
                            fraction: 0.25,
                        },
                        y: PaintCoordinate {
                            length: 2.0,
                            fraction: 0.75,
                        },
                    },
                },
            ))
        );
    }

    style.clip_path = Some(ComputedClipPath {
        reference_box: ClipBoxValue::Border,
        shape: ComputedClipShape::Inset {
            offsets: Edges {
                top: ComputedLengthPercentage::new(1.0, 0.1),
                right: ComputedLengthPercentage::new(2.0, 0.2),
                bottom: ComputedLengthPercentage::new(3.0, 0.3),
                left: ComputedLengthPercentage::new(4.0, 0.4),
            },
            radii: Corners {
                top_left: radius(1.0, 0.1, 2.0, 0.2),
                top_right: radius(3.0, 0.3, 4.0, 0.4),
                bottom_right: radius(5.0, 0.5, 6.0, 0.6),
                bottom_left: radius(7.0, 0.7, 8.0, 0.8),
            },
        },
    });
    let Some((PaintBox::Border, ClipShape::Inset { edges, radii })) =
        lower_paint(&style, &layout).visual_effects.clip_path
    else {
        panic!("expected an inset border-box clip");
    };
    assert_eq!(
        edges.top,
        PaintCoordinate {
            length: 1.0,
            fraction: 0.1
        }
    );
    assert_eq!(
        edges.left,
        PaintCoordinate {
            length: 4.0,
            fraction: 0.4
        }
    );
    assert_eq!(radii.top_right.horizontal.length, 3.0);
    assert_eq!(radii.bottom_left.vertical.fraction, 0.8);

    style.clip_path = Some(ComputedClipPath {
        reference_box: ClipBoxValue::Content,
        shape: ComputedClipShape::Ellipse {
            radius_x: ComputedLengthPercentage::new(10.0, 0.1),
            radius_y: ComputedLengthPercentage::new(20.0, 0.2),
            center,
        },
    });
    let Some((
        PaintBox::Content,
        ClipShape::Ellipse {
            radius_x,
            radius_y,
            center: actual_center,
        },
    )) = lower_paint(&style, &layout).visual_effects.clip_path
    else {
        panic!("expected an ellipse content-box clip");
    };
    assert_eq!(
        radius_x,
        PaintLengthPercentage {
            length: 10.0,
            fraction: 0.1
        }
    );
    assert_eq!(
        radius_y,
        PaintLengthPercentage {
            length: 20.0,
            fraction: 0.2
        }
    );
    assert_eq!(actual_center.x.fraction, 0.25);

    let commands = vec![
        ComputedClipPathCommand::MoveTo(point(1.0, 0.1, 2.0, 0.2)),
        ComputedClipPathCommand::LineTo(point(3.0, 0.3, 4.0, 0.4)),
        ComputedClipPathCommand::QuadraticTo {
            control: point(5.0, 0.5, 6.0, 0.6),
            end: point(7.0, 0.7, 8.0, 0.8),
        },
        ComputedClipPathCommand::CubicTo {
            control_1: point(9.0, 0.9, 10.0, 1.0),
            control_2: point(11.0, 1.1, 12.0, 1.2),
            end: point(13.0, 1.3, 14.0, 1.4),
        },
        ComputedClipPathCommand::Close,
    ];
    for (fill_rule, expected) in [
        (ClipFillRuleValue::NonZero, FillRule::NonZero),
        (ClipFillRuleValue::EvenOdd, FillRule::EvenOdd),
    ] {
        style.clip_path = Some(ComputedClipPath {
            reference_box: ClipBoxValue::View,
            shape: ComputedClipShape::Path {
                fill_rule,
                commands: commands.clone(),
            },
        });
        let Some((
            PaintBox::View,
            ClipShape::Path {
                fill_rule: actual,
                commands,
            },
        )) = lower_paint(&style, &layout).visual_effects.clip_path
        else {
            panic!("expected a view-box path clip");
        };
        assert_eq!(actual, expected);
        assert_eq!(commands.len(), 5);
        assert!(matches!(commands[0], PathCommand::MoveTo(_)));
        assert!(matches!(commands[1], PathCommand::LineTo(_)));
        assert!(matches!(commands[2], PathCommand::QuadraticTo { .. }));
        assert!(matches!(commands[3], PathCommand::CubicTo { .. }));
        assert_eq!(commands[4], PathCommand::Close);
    }
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
fn prepends_current_node_perspective_to_the_transform_matrix() {
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

    let invalid_offset = OffsetPathValue::Inset(Box::new(whisker_style::ComputedInsetPathValue {
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
