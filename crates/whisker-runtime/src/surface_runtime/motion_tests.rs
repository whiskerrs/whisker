use super::*;

fn opacity_animation(
    delay_ms: f32,
    iterations: MotionIterationCount,
    direction: MotionDirection,
    fill_mode: MotionFillMode,
    play_state: MotionPlayState,
) -> ActiveKeyframeAnimation {
    let mut animation = ActiveKeyframeAnimation {
        declaration: AnimationValue {
            name: Some("test".to_owned()),
            keyframes: None,
            duration: whisker_engine::whisker_style::MotionTime::milliseconds(100.0),
            easing: MotionEasing::Linear,
            delay: whisker_engine::whisker_style::MotionTime::milliseconds(delay_ms),
            iteration_count: iterations,
            direction,
            fill_mode,
            play_state,
        },
        tracks: vec![KeyframePropertyTrack {
            property: StyleProperty::Opacity,
            points: vec![
                KeyframePoint {
                    offset: 0.0,
                    value: AnimatedPropertyValue::Number(0.0),
                    easing: None,
                },
                KeyframePoint {
                    offset: 1.0,
                    value: AnimatedPropertyValue::Number(1.0),
                    easing: None,
                },
            ],
        }],
        current_time_ms: 0.0,
        last_timestamp_ms: None,
        current: HashMap::new(),
        finished: false,
        sampled_progress: None,
        start_emitted: false,
        completed_iterations: 0,
        end_emitted: false,
    };
    animation.sample_current_time();
    animation
}

fn opacity_sample(animation: &ActiveKeyframeAnimation) -> Option<f32> {
    match animation.current.get(&StyleProperty::Opacity) {
        Some(AnimatedPropertyValue::Number(value)) => Some(*value),
        _ => None,
    }
}

#[test]
fn keyframe_timeline_honors_delay_fill_iterations_and_direction() {
    let mut animation = opacity_animation(
        50.0,
        MotionIterationCount::Count(StyleNumber::new(2.0)),
        MotionDirection::Alternate,
        MotionFillMode::Both,
        MotionPlayState::Running,
    );
    assert_eq!(opacity_sample(&animation), Some(0.0));

    animation.sample(1_000.0);
    animation.sample(1_100.0);
    assert_eq!(opacity_sample(&animation), Some(0.5));
    animation.sample(1_200.0);
    assert_eq!(opacity_sample(&animation), Some(0.5));
    animation.sample(1_250.0);
    assert_eq!(opacity_sample(&animation), Some(0.0));
    assert!(animation.finished);
    assert!(!animation.needs_frame());
}

#[test]
fn paused_keyframe_timeline_keeps_its_hold_time_when_resumed() {
    let mut animation = opacity_animation(
        0.0,
        MotionIterationCount::Count(StyleNumber::new(1.0)),
        MotionDirection::Normal,
        MotionFillMode::Forwards,
        MotionPlayState::Running,
    );
    animation.sample(1_000.0);
    animation.sample(1_040.0);
    assert_eq!(opacity_sample(&animation), Some(0.4));

    animation.declaration.play_state = MotionPlayState::Paused;
    animation.sample(5_000.0);
    assert_eq!(opacity_sample(&animation), Some(0.4));
    assert!(!animation.needs_frame());

    animation.declaration.play_state = MotionPlayState::Running;
    animation.last_timestamp_ms = None;
    animation.sample(9_000.0);
    assert_eq!(opacity_sample(&animation), Some(0.4));
    animation.sample(9_010.0);
    assert_eq!(opacity_sample(&animation), Some(0.5));
}

#[test]
fn negative_delay_seeks_into_the_first_iteration() {
    let mut animation = opacity_animation(
        -25.0,
        MotionIterationCount::Count(StyleNumber::new(1.0)),
        MotionDirection::Normal,
        MotionFillMode::None,
        MotionPlayState::Running,
    );
    assert_eq!(opacity_sample(&animation), Some(0.25));
    animation.sample(10.0);
    animation.sample(35.0);
    assert_eq!(opacity_sample(&animation), Some(0.5));
}

fn hsla(hue_degrees: f32) -> PaintColor {
    PaintColor::Hsla {
        hue_degrees,
        saturation: 100.0,
        lightness: 50.0,
        alpha: 1.0,
    }
}

#[test]
fn hsl_and_transparent_colors_canonicalize_to_srgb() {
    for (hue, expected) in [
        (0.0, (255, 0, 0)),
        (60.0, (255, 255, 0)),
        (120.0, (0, 255, 0)),
        (180.0, (0, 255, 255)),
        (240.0, (0, 0, 255)),
        (300.0, (255, 0, 255)),
        (360.0, (255, 0, 0)),
    ] {
        let color = RgbaColor::from_paint(&hsla(hue)).unwrap().into_paint();
        assert_eq!(
            color,
            PaintColor::Srgba {
                red: expected.0,
                green: expected.1,
                blue: expected.2,
                alpha: 1.0,
            }
        );
    }
    assert!(RgbaColor::from_paint(&PaintColor::Named("red".into())).is_none());
    assert_eq!(
        RgbaColor::from_paint(&PaintColor::Named("TRANSPARENT".into()))
            .unwrap()
            .into_paint(),
        PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        }
    );
}

#[test]
fn color_interpolation_uses_premultiplied_alpha() {
    let transparent_red = RgbaColor {
        red: 1.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };
    let opaque_blue = RgbaColor {
        red: 0.0,
        green: 0.0,
        blue: 1.0,
        alpha: 1.0,
    };
    assert_eq!(
        transparent_red.interpolate(opaque_blue, 0.5).into_paint(),
        PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 255,
            alpha: 0.5,
        }
    );
    assert_eq!(
        transparent_red
            .interpolate(transparent_red, 0.5)
            .into_paint(),
        PaintColor::Srgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0.0,
        }
    );
}

#[test]
fn compatible_transform_functions_interpolate_with_identity_padding() {
    let number = StyleNumber::new;
    let length = |value| ComputedLengthPercentage::new(value, value / 100.0);
    let cases = [
        (
            ComputedTransformFunction::Translate {
                x: length(0.0),
                y: length(10.0),
                z: number(20.0),
            },
            ComputedTransformFunction::Translate {
                x: length(100.0),
                y: length(30.0),
                z: number(40.0),
            },
        ),
        (
            ComputedTransformFunction::RotateX(number(0.0)),
            ComputedTransformFunction::RotateX(number(90.0)),
        ),
        (
            ComputedTransformFunction::RotateY(number(0.0)),
            ComputedTransformFunction::RotateY(number(90.0)),
        ),
        (
            ComputedTransformFunction::RotateZ(number(0.0)),
            ComputedTransformFunction::RotateZ(number(90.0)),
        ),
        (
            ComputedTransformFunction::Scale {
                x: number(1.0),
                y: number(2.0),
                z: number(3.0),
            },
            ComputedTransformFunction::Scale {
                x: number(3.0),
                y: number(4.0),
                z: number(5.0),
            },
        ),
        (
            ComputedTransformFunction::Skew {
                x_degrees: number(0.0),
                y_degrees: number(10.0),
            },
            ComputedTransformFunction::Skew {
                x_degrees: number(20.0),
                y_degrees: number(30.0),
            },
        ),
    ];
    for (from, to) in cases {
        assert!(interpolate_transform_function(&from, &to, 0.5).is_some());
        let from = ComputedTransformStyle {
            functions: vec![from],
            ..ComputedTransformStyle::default()
        };
        let to = ComputedTransformStyle {
            functions: vec![to],
            origin_x: ComputedLengthPercentage::new(12.0, 0.0),
            ..ComputedTransformStyle::default()
        };
        let current = interpolate_transform_style(&from, &to, 0.5, 100.0, 100.0).unwrap();
        assert_eq!(current.origin_x, to.origin_x);
        assert_eq!(current.functions.len(), 1);
        assert!(
                interpolate_transform_style(
                    &ComputedTransformStyle::default(),
                    &to,
                    0.5,
                    100.0,
                    100.0,
                )
                .is_some()
            );
        assert!(
                interpolate_transform_style(
                    &to,
                    &ComputedTransformStyle::default(),
                    0.5,
                    100.0,
                    100.0,
                )
                .is_some()
            );
    }
}

#[test]
fn incompatible_and_matrix_transform_functions_use_decomposition() {
    let number = StyleNumber::new;
    let matrix = ComputedTransformFunction::Matrix(
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
        .map(number),
    );
    assert!(identity_transform_function(&matrix).is_none());
    assert!(
        interpolate_transform_function(
            &matrix,
            &ComputedTransformFunction::Matrix([number(2.0); 16]),
            0.5,
        )
        .is_none()
    );
    assert!(
        interpolate_transform_function(
            &ComputedTransformFunction::RotateX(number(0.0)),
            &ComputedTransformFunction::RotateY(number(90.0)),
            0.5,
        )
        .is_none()
    );
    let matrix_style = ComputedTransformStyle {
        functions: vec![matrix],
        ..ComputedTransformStyle::default()
    };
    assert!(
        interpolate_transform_style(
            &ComputedTransformStyle::default(),
            &matrix_style,
            0.5,
            100.0,
            100.0,
        )
        .is_some()
    );
}
