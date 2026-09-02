use super::*;
use crate::{
    BackdropFilterValue, BackgroundLayerValue, BackgroundRepeatValue, BackgroundValue,
    BorderRadiusValue, BoxShadowValue, ClipBoxValue, ClipFillRuleValue, ClipPathCommandValue,
    ClipPathValue, ClipPointValue, ClipShapeValue, GradientStopValue, LengthPercentageValue,
    LengthUnit, LengthValue,
};

#[test]
fn unresolved_composite_component_is_rejected_before_paint_lowering() {
    let name = crate::CustomPropertyName::new("--value").unwrap();
    let value =
        ComponentValue::<ColorValue>::Variable(crate::CustomPropertyReference::new(name.clone()));
    assert!(std::panic::catch_unwind(|| component(&value)).is_err());
    let value =
        ComponentValue::<LengthValue>::Variable(crate::CustomPropertyReference::new(name.clone()));
    assert!(std::panic::catch_unwind(|| component(&value)).is_err());
    let value = ComponentValue::<StyleNumber>::Variable(crate::CustomPropertyReference::new(name));
    assert!(std::panic::catch_unwind(|| component(&value)).is_err());
}

fn number(value: f32) -> StyleNumber {
    StyleNumber::new(value)
}

fn px_length(value: f32) -> LengthPercentageValue {
    LengthPercentageValue::Length(LengthValue::Dimension {
        value: number(value),
        unit: LengthUnit::Px,
    })
}

fn px(value: f32) -> StyleValue {
    StyleValue::LengthPercentage(px_length(value))
}

fn percentage(value: f32) -> LengthPercentageValue {
    LengthPercentageValue::Percentage(number(value))
}

#[test]
fn backdrop_blur_resolves_relative_lengths_and_rejects_negative_radii() {
    let blur = |value| {
        StyleValue::BackdropFilter(BackdropFilterValue::Blur(
            LengthValue::Dimension {
                value: number(value),
                unit: LengthUnit::Rem,
            }
            .into(),
        ))
    };
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, blur(2.0)),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().paint().backdrop_blur,
        Some(number(28.0))
    );
    assert_eq!(
        crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, blur(-1.0)),
            None,
            StyleEnvironment::default(),
        ),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::BackdropFilter
        ))
    );
    assert_eq!(
        crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::BackdropFilter,
                StyleValue::BackdropFilter(BackdropFilterValue::None),
            ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap()
        .computed()
        .paint()
        .backdrop_blur,
        None
    );
    for value in [
        StyleValue::Number(number(1.0)),
        StyleValue::BackdropFilter(BackdropFilterValue::Blur(
            LengthValue::Dimension {
                value: number(f32::NAN),
                unit: LengthUnit::Px,
            }
            .into(),
        )),
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::BackdropFilter, value),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BackdropFilter
            ))
        );
    }
}

#[test]
fn box_shadow_resolves_every_component_and_rejects_invalid_values() {
    let length = |value| {
        ComponentValue::Value(LengthValue::Dimension {
            value: number(value),
            unit: LengthUnit::Px,
        })
    };
    let shadow = BoxShadowValue {
        offset_x: length(1.0),
        offset_y: length(-2.0),
        blur_radius: length(3.0),
        spread_radius: length(-4.0),
        color: ComponentValue::Value(ColorValue::Named("shadow".into())),
        inset: true,
    };
    let resolve = |value| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::BoxShadow, value),
            None,
            StyleEnvironment::default(),
        )
    };
    let resolved = resolve(StyleValue::BoxShadows(vec![shadow.clone()])).unwrap();
    assert_eq!(
        resolved.computed().paint().box_shadows,
        [ComputedBoxShadow {
            offset_x: number(1.0),
            offset_y: number(-2.0),
            blur_radius: number(3.0),
            spread_radius: number(-4.0),
            color: ColorValue::Named("shadow".into()),
            inset: true,
        }]
    );

    let expected = Err(StyleResolutionError::InvalidPropertyValue(
        StyleProperty::BoxShadow,
    ));
    assert_eq!(resolve(StyleValue::Number(number(1.0))), expected);

    let mut negative_blur = shadow.clone();
    negative_blur.blur_radius = length(-1.0);
    assert_eq!(
        resolve(StyleValue::BoxShadows(vec![negative_blur])),
        expected
    );

    for field in 0..4 {
        let mut invalid = shadow.clone();
        let invalid_length = length(f32::NAN);
        match field {
            0 => invalid.offset_x = invalid_length,
            1 => invalid.offset_y = invalid_length,
            2 => invalid.blur_radius = invalid_length,
            3 => invalid.spread_radius = invalid_length,
            _ => unreachable!(),
        }
        assert_eq!(resolve(StyleValue::BoxShadows(vec![invalid])), expected);
    }
}

#[test]
fn clip_path_resolves_every_shape_command_and_error_position() {
    fn point(x: LengthPercentageValue, y: LengthPercentageValue) -> ClipPointValue {
        ClipPointValue { x, y }
    }

    let invalid = || px_length(f32::NAN);
    let radius = |horizontal, vertical| BorderRadiusValue {
        horizontal,
        vertical,
    };
    let resolve = |value| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::ClipPath, value),
            None,
            StyleEnvironment::default(),
        )
    };
    let clip = |reference_box, shape| {
        StyleValue::ClipPath(ClipPathValue::Shape {
            reference_box,
            shape,
        })
    };
    let expected = Err(StyleResolutionError::InvalidPropertyValue(
        StyleProperty::ClipPath,
    ));

    assert_eq!(
        resolve(StyleValue::ClipPath(ClipPathValue::None))
            .unwrap()
            .computed()
            .paint()
            .clip_path,
        None
    );
    assert_eq!(resolve(StyleValue::Number(number(1.0))), expected);

    let inset_offsets = [
        percentage(10.0),
        px_length(2.0),
        percentage(30.0),
        px_length(4.0),
    ];
    let inset_radii = [
        radius(px_length(1.0), percentage(10.0)),
        radius(px_length(2.0), percentage(20.0)),
        radius(px_length(3.0), percentage(30.0)),
        radius(px_length(4.0), percentage(40.0)),
    ];
    let resolved = resolve(clip(
        ClipBoxValue::Padding,
        ClipShapeValue::Inset {
            offsets: inset_offsets.clone(),
            radii: Some(inset_radii.clone()),
        },
    ))
    .unwrap();
    assert!(matches!(
        resolved.computed().paint().clip_path,
        Some(ComputedClipPath {
            reference_box: ClipBoxValue::Padding,
            shape: ComputedClipShape::Inset { .. },
        })
    ));
    let without_radii = resolve(clip(
        ClipBoxValue::Border,
        ClipShapeValue::Inset {
            offsets: inset_offsets.clone(),
            radii: None,
        },
    ))
    .unwrap();
    let Some(ComputedClipPath {
        shape: ComputedClipShape::Inset { radii, .. },
        ..
    }) = &without_radii.computed().paint().clip_path
    else {
        unreachable!();
    };
    assert_eq!(radii, &Corners::all(ComputedCornerRadius::ZERO));

    for index in 0..4 {
        let mut offsets = inset_offsets.clone();
        offsets[index] = invalid();
        assert_eq!(
            resolve(clip(
                ClipBoxValue::Border,
                ClipShapeValue::Inset {
                    offsets,
                    radii: None,
                },
            )),
            expected
        );
    }
    for index in 0..8 {
        let mut radii = inset_radii.clone();
        if index % 2 == 0 {
            radii[index / 2].horizontal = invalid();
        } else {
            radii[index / 2].vertical = invalid();
        }
        assert_eq!(
            resolve(clip(
                ClipBoxValue::Border,
                ClipShapeValue::Inset {
                    offsets: inset_offsets.clone(),
                    radii: Some(radii),
                },
            )),
            expected
        );
    }

    let circle = ClipShapeValue::Circle {
        radius: percentage(25.0),
        center_x: px_length(5.0),
        center_y: percentage(75.0),
    };
    assert!(matches!(
        resolve(clip(ClipBoxValue::Fill, circle.clone()))
            .unwrap()
            .computed()
            .paint()
            .clip_path,
        Some(ComputedClipPath {
            reference_box: ClipBoxValue::Fill,
            shape: ComputedClipShape::Circle { .. },
        })
    ));
    for index in 0..3 {
        let mut shape = circle.clone();
        let ClipShapeValue::Circle {
            radius,
            center_x,
            center_y,
        } = &mut shape
        else {
            unreachable!();
        };
        *match index {
            0 => radius,
            1 => center_x,
            2 => center_y,
            _ => unreachable!(),
        } = invalid();
        assert_eq!(resolve(clip(ClipBoxValue::Fill, shape)), expected);
    }

    let ellipse = ClipShapeValue::Ellipse {
        radius_x: px_length(10.0),
        radius_y: percentage(20.0),
        center_x: percentage(30.0),
        center_y: px_length(40.0),
    };
    assert!(matches!(
        resolve(clip(ClipBoxValue::Stroke, ellipse.clone()))
            .unwrap()
            .computed()
            .paint()
            .clip_path,
        Some(ComputedClipPath {
            reference_box: ClipBoxValue::Stroke,
            shape: ComputedClipShape::Ellipse { .. },
        })
    ));
    for index in 0..4 {
        let mut shape = ellipse.clone();
        let ClipShapeValue::Ellipse {
            radius_x,
            radius_y,
            center_x,
            center_y,
        } = &mut shape
        else {
            unreachable!();
        };
        *match index {
            0 => radius_x,
            1 => radius_y,
            2 => center_x,
            3 => center_y,
            _ => unreachable!(),
        } = invalid();
        assert_eq!(resolve(clip(ClipBoxValue::Stroke, shape)), expected);
    }

    let path_commands = vec![
        ClipPathCommandValue::MoveTo(point(px_length(1.0), percentage(10.0))),
        ClipPathCommandValue::LineTo(point(px_length(2.0), percentage(20.0))),
        ClipPathCommandValue::QuadraticTo {
            control: point(px_length(3.0), percentage(30.0)),
            end: point(px_length(4.0), percentage(40.0)),
        },
        ClipPathCommandValue::CubicTo {
            control_1: point(px_length(5.0), percentage(50.0)),
            control_2: point(px_length(6.0), percentage(60.0)),
            end: point(px_length(7.0), percentage(70.0)),
        },
        ClipPathCommandValue::Close,
    ];
    for fill_rule in [ClipFillRuleValue::NonZero, ClipFillRuleValue::EvenOdd] {
        let resolved = resolve(clip(
            ClipBoxValue::View,
            ClipShapeValue::Path {
                fill_rule,
                commands: path_commands.clone(),
            },
        ))
        .unwrap();
        let Some(ComputedClipPath {
            reference_box: ClipBoxValue::View,
            shape:
                ComputedClipShape::Path {
                    fill_rule: actual,
                    commands,
                },
        }) = &resolved.computed().paint().clip_path
        else {
            unreachable!();
        };
        assert_eq!(*actual, fill_rule);
        assert_eq!(commands.len(), 5);
    }

    for command_index in 0..4 {
        let coordinate_count = match command_index {
            0 | 1 => 2,
            2 => 4,
            3 => 6,
            _ => unreachable!(),
        };
        for coordinate_index in 0..coordinate_count {
            let mut commands = path_commands.clone();
            let points: Vec<&mut ClipPointValue> = match &mut commands[command_index] {
                ClipPathCommandValue::MoveTo(point) | ClipPathCommandValue::LineTo(point) => {
                    vec![point]
                }
                ClipPathCommandValue::QuadraticTo { control, end } => vec![control, end],
                ClipPathCommandValue::CubicTo {
                    control_1,
                    control_2,
                    end,
                } => vec![control_1, control_2, end],
                ClipPathCommandValue::Close => unreachable!(),
            };
            let point = &mut *points.into_iter().nth(coordinate_index / 2).unwrap();
            if coordinate_index % 2 == 0 {
                point.x = invalid();
            } else {
                point.y = invalid();
            }
            assert_eq!(
                resolve(clip(
                    ClipBoxValue::View,
                    ClipShapeValue::Path {
                        fill_rule: ClipFillRuleValue::EvenOdd,
                        commands,
                    },
                )),
                expected
            );
        }
    }
}

#[test]
fn transform_retains_box_percentages_and_three_dimensional_functions() {
    let transform = StyleValue::Transform(TransformValue(vec![
        TransformFunctionValue::Translate(
            LengthPercentageValue::Percentage(number(50.0)),
            px_length(4.0),
        ),
        TransformFunctionValue::Scale(number(2.0).into(), number(3.0).into()),
    ]));
    let origin = StyleValue::TransformOrigin(TransformOriginValue {
        horizontal: LengthPercentageValue::Percentage(number(25.0)),
        vertical: LengthPercentageValue::Percentage(number(75.0)),
    });
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new()
            .push(StyleProperty::Transform, transform)
            .push(StyleProperty::TransformOrigin, origin),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    let transform = &resolved.computed().paint().transform;
    assert_eq!(transform.origin_x, ComputedLengthPercentage::new(0.0, 0.25));
    assert_eq!(transform.origin_y, ComputedLengthPercentage::new(0.0, 0.75));
    assert_eq!(
        transform.functions,
        [
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::new(0.0, 0.5),
                y: ComputedLengthPercentage::new(4.0, 0.0),
                z: number(0.0),
            },
            ComputedTransformFunction::Scale {
                x: number(2.0),
                y: number(3.0),
                z: number(1.0),
            },
        ]
    );

    let rotated = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Transform,
            StyleValue::Transform(TransformValue(vec![
                TransformFunctionValue::RotateX(number(30.0).into()),
                TransformFunctionValue::TranslateZ(
                    LengthValue::Dimension {
                        value: number(8.0),
                        unit: LengthUnit::Px,
                    }
                    .into(),
                ),
            ])),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        rotated.computed().paint().transform.functions,
        [
            ComputedTransformFunction::RotateX(number(30.0)),
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::ZERO,
                y: ComputedLengthPercentage::ZERO,
                z: number(8.0),
            },
        ]
    );
}

#[test]
fn transform_resolves_every_function_and_rejects_invalid_inputs() {
    let length = |value| LengthValue::Dimension {
        value: number(value),
        unit: LengthUnit::Px,
    };
    let mut matrix_3d = [number(0.0); 16];
    for index in [0, 5, 10, 15] {
        matrix_3d[index] = number(1.0);
    }
    let functions = vec![
        TransformFunctionValue::TranslateX(percentage(10.0)),
        TransformFunctionValue::TranslateY(px_length(2.0)),
        TransformFunctionValue::TranslateZ(LengthValue::Zero.into()),
        TransformFunctionValue::Translate3d(
            px_length(3.0),
            percentage(20.0),
            LengthValue::Zero.into(),
        ),
        TransformFunctionValue::Rotate(number(10.0).into()),
        TransformFunctionValue::RotateX(number(0.0).into()),
        TransformFunctionValue::RotateY(number(0.0).into()),
        TransformFunctionValue::RotateZ(number(20.0).into()),
        TransformFunctionValue::ScaleX(number(2.0).into()),
        TransformFunctionValue::ScaleY(number(3.0).into()),
        TransformFunctionValue::Skew(number(4.0).into(), number(5.0).into()),
        TransformFunctionValue::SkewX(number(6.0).into()),
        TransformFunctionValue::SkewY(number(7.0).into()),
        TransformFunctionValue::Matrix([
            number(1.0),
            number(2.0),
            number(3.0),
            number(4.0),
            number(5.0),
            number(6.0),
        ]),
        TransformFunctionValue::Matrix3d(matrix_3d),
    ];
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Transform,
            StyleValue::Transform(TransformValue(functions)),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().paint().transform.functions,
        [
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::new(0.0, 0.1),
                y: ComputedLengthPercentage::ZERO,
                z: number(0.0),
            },
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::ZERO,
                y: ComputedLengthPercentage::new(2.0, 0.0),
                z: number(0.0),
            },
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::ZERO,
                y: ComputedLengthPercentage::ZERO,
                z: number(0.0),
            },
            ComputedTransformFunction::Translate {
                x: ComputedLengthPercentage::new(3.0, 0.0),
                y: ComputedLengthPercentage::new(0.0, 0.2),
                z: number(0.0),
            },
            ComputedTransformFunction::RotateZ(number(10.0)),
            ComputedTransformFunction::RotateX(number(0.0)),
            ComputedTransformFunction::RotateY(number(0.0)),
            ComputedTransformFunction::RotateZ(number(20.0)),
            ComputedTransformFunction::Scale {
                x: number(2.0),
                y: number(1.0),
                z: number(1.0),
            },
            ComputedTransformFunction::Scale {
                x: number(1.0),
                y: number(3.0),
                z: number(1.0),
            },
            ComputedTransformFunction::Skew {
                x_degrees: number(4.0),
                y_degrees: number(5.0),
            },
            ComputedTransformFunction::Skew {
                x_degrees: number(6.0),
                y_degrees: number(0.0),
            },
            ComputedTransformFunction::Skew {
                x_degrees: number(0.0),
                y_degrees: number(7.0),
            },
            ComputedTransformFunction::Matrix([
                number(1.0),
                number(2.0),
                number(0.0),
                number(0.0),
                number(3.0),
                number(4.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(0.0),
                number(1.0),
                number(0.0),
                number(5.0),
                number(6.0),
                number(0.0),
                number(1.0),
            ]),
            ComputedTransformFunction::Matrix(matrix_3d),
        ]
    );

    let invalid_transform = |function| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::Transform,
                StyleValue::Transform(TransformValue(vec![function])),
            ),
            None,
            StyleEnvironment::default(),
        )
    };
    let mut non_finite_matrix = [number(0.0); 6];
    non_finite_matrix[0] = number(f32::NAN);
    let mut non_finite_matrix_3d = matrix_3d;
    non_finite_matrix_3d[0] = number(f32::INFINITY);
    let mut spatial_matrix_3d = matrix_3d;
    spatial_matrix_3d[14] = number(1.0);
    for function in [
        TransformFunctionValue::Translate(
            LengthPercentageValue::Length(length(f32::NAN)),
            px_length(0.0),
        ),
        TransformFunctionValue::Translate(
            px_length(0.0),
            LengthPercentageValue::Length(length(f32::NAN)),
        ),
        TransformFunctionValue::TranslateX(LengthPercentageValue::Length(length(f32::NAN))),
        TransformFunctionValue::TranslateY(LengthPercentageValue::Length(length(f32::NAN))),
        TransformFunctionValue::TranslateZ(length(f32::NAN).into()),
        TransformFunctionValue::Translate3d(
            px_length(0.0),
            px_length(0.0),
            length(f32::NAN).into(),
        ),
        TransformFunctionValue::Translate3d(
            LengthPercentageValue::Length(length(f32::NAN)),
            px_length(0.0),
            LengthValue::Zero.into(),
        ),
        TransformFunctionValue::Translate3d(
            px_length(0.0),
            LengthPercentageValue::Length(length(f32::NAN)),
            LengthValue::Zero.into(),
        ),
        TransformFunctionValue::Rotate(number(f32::NAN).into()),
        TransformFunctionValue::RotateX(number(f32::NAN).into()),
        TransformFunctionValue::RotateY(number(f32::NAN).into()),
        TransformFunctionValue::Scale(number(f32::NAN).into(), number(1.0).into()),
        TransformFunctionValue::Scale(number(1.0).into(), number(f32::NAN).into()),
        TransformFunctionValue::ScaleX(number(f32::INFINITY).into()),
        TransformFunctionValue::ScaleY(number(f32::INFINITY).into()),
        TransformFunctionValue::Skew(number(f32::NAN).into(), number(0.0).into()),
        TransformFunctionValue::Skew(number(0.0).into(), number(f32::NAN).into()),
        TransformFunctionValue::SkewX(number(f32::NAN).into()),
        TransformFunctionValue::SkewY(number(f32::NAN).into()),
        TransformFunctionValue::Matrix(non_finite_matrix),
        TransformFunctionValue::Matrix3d(non_finite_matrix_3d),
    ] {
        assert_eq!(
            invalid_transform(function),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::Transform
            ))
        );
    }
    for function in [
        TransformFunctionValue::TranslateZ(length(1.0).into()),
        TransformFunctionValue::Translate3d(px_length(0.0), px_length(0.0), length(1.0).into()),
        TransformFunctionValue::RotateX(number(1.0).into()),
        TransformFunctionValue::RotateY(number(1.0).into()),
        TransformFunctionValue::Matrix3d(spatial_matrix_3d),
    ] {
        assert!(invalid_transform(function).is_ok());
    }

    for (property, value) in [
        (StyleProperty::Transform, StyleValue::Number(number(1.0))),
        (
            StyleProperty::TransformOrigin,
            StyleValue::Number(number(1.0)),
        ),
        (
            StyleProperty::TransformOrigin,
            StyleValue::TransformOrigin(TransformOriginValue {
                horizontal: LengthPercentageValue::Length(length(f32::NAN)),
                vertical: px_length(0.0),
            }),
        ),
        (
            StyleProperty::TransformOrigin,
            StyleValue::TransformOrigin(TransformOriginValue {
                horizontal: px_length(0.0),
                vertical: LengthPercentageValue::Length(length(f32::NAN)),
            }),
        ),
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(property, value),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
}

#[test]
fn perspective_resolves_absolute_length_and_rejects_negative_distance() {
    let perspective = |value| {
        StyleValue::Length(LengthValue::Dimension {
            value: number(value),
            unit: LengthUnit::Rem,
        })
    };
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new().push(StyleProperty::Perspective, perspective(2.0)),
        None,
        StyleEnvironment::new(320.0, 480.0, 2.0, 16.0),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().paint().transform.perspective,
        Some(number(32.0))
    );
    assert_eq!(
        crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::Perspective, perspective(-1.0)),
            None,
            StyleEnvironment::default(),
        ),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::Perspective
        ))
    );
    for value in [StyleValue::Number(number(1.0)), perspective(f32::NAN)] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(StyleProperty::Perspective, value),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::Perspective
            ))
        );
    }
}

#[test]
fn motion_path_resolves_progress_and_rotation_and_rejects_invalid_values() {
    let point = |x, y| crate::MotionPathPointValue {
        x: number(x),
        y: number(y),
    };
    let commands = vec![
        MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
        MotionPathCommandValue::LineTo(point(40.0, 0.0)),
        MotionPathCommandValue::QuadraticTo {
            control: point(50.0, 10.0),
            to: point(60.0, 0.0),
        },
        MotionPathCommandValue::CubicTo {
            control1: point(70.0, -10.0),
            control2: point(80.0, 10.0),
            to: point(90.0, 0.0),
        },
        MotionPathCommandValue::ArcTo {
            radius_x: number(25.0),
            radius_y: number(10.0),
            x_axis_rotation: number(30.0),
            large_arc: true,
            sweep: false,
            to: point(100.0, 20.0),
        },
    ];
    let path = OffsetPathValue::Path(commands.clone());
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new()
            .push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(path.clone()),
            )
            .push(
                StyleProperty::OffsetDistance,
                StyleValue::LengthPercentage(percentage(75.0)),
            )
            .push(
                StyleProperty::OffsetRotate,
                StyleValue::OffsetRotate(OffsetRotateValue::Auto),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    let transform = &resolved.computed().paint().transform;
    assert_eq!(
        transform.offset_path,
        ComputedOffsetPathValue::Path(commands)
    );
    assert_eq!(transform.offset_distance, number(0.75));
    assert_eq!(transform.offset_rotate, OffsetRotateValue::Auto);

    let fixed = crate::resolve_style(
        &SpecifiedStyle::new()
            .push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::None),
            )
            .push(
                StyleProperty::OffsetDistance,
                StyleValue::Number(number(0.5)),
            )
            .push(
                StyleProperty::OffsetRotate,
                StyleValue::OffsetRotate(OffsetRotateValue::Angle(number(45.0))),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        fixed.computed().paint().transform.offset_path,
        ComputedOffsetPathValue::None
    );
    assert_eq!(
        fixed.computed().paint().transform.offset_distance,
        number(0.5)
    );
    assert_eq!(
        fixed.computed().paint().transform.offset_rotate,
        OffsetRotateValue::Angle(number(45.0))
    );

    let circle = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::OffsetPath,
            StyleValue::OffsetPath(OffsetPathValue::Circle {
                radius: percentage(25.0),
                center_x: px_length(10.0),
                center_y: percentage(75.0),
            }),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        circle.computed().paint().transform.offset_path,
        ComputedOffsetPathValue::Circle {
            radius: ComputedLengthPercentage::new(0.0, 0.25),
            center_x: ComputedLengthPercentage::new(10.0, 0.0),
            center_y: ComputedLengthPercentage::new(0.0, 0.75),
        }
    );

    let ellipse = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::OffsetPath,
            StyleValue::OffsetPath(OffsetPathValue::Ellipse {
                radius_x: px_length(10.0),
                radius_y: percentage(25.0),
                center_x: percentage(50.0),
                center_y: percentage(50.0),
            }),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        ellipse.computed().paint().transform.offset_path,
        ComputedOffsetPathValue::Ellipse {
            radius_x: ComputedLengthPercentage::new(10.0, 0.0),
            radius_y: ComputedLengthPercentage::new(0.0, 0.25),
            center_x: ComputedLengthPercentage::new(0.0, 0.5),
            center_y: ComputedLengthPercentage::new(0.0, 0.5),
        }
    );

    let inset_radius = |horizontal, vertical| BorderRadiusValue {
        horizontal,
        vertical,
    };
    let inset = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::OffsetPath,
            StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(crate::InsetPathValue {
                offsets: [
                    percentage(10.0),
                    px_length(20.0),
                    percentage(25.0),
                    px_length(5.0),
                ],
                radii: Some([
                    inset_radius(px_length(2.0), percentage(5.0)),
                    inset_radius(px_length(3.0), percentage(6.0)),
                    inset_radius(px_length(4.0), percentage(7.0)),
                    inset_radius(px_length(5.0), percentage(8.0)),
                ]),
            }))),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        inset.computed().paint().transform.offset_path,
        ComputedOffsetPathValue::Inset(Box::new(ComputedInsetPathValue {
            offsets: Edges {
                top: ComputedLengthPercentage::new(0.0, 0.1),
                right: ComputedLengthPercentage::new(20.0, 0.0),
                bottom: ComputedLengthPercentage::new(0.0, 0.25),
                left: ComputedLengthPercentage::new(5.0, 0.0),
            },
            radii: Some(Corners {
                top_left: ComputedCornerRadius {
                    horizontal: ComputedLengthPercentage::new(2.0, 0.0),
                    vertical: ComputedLengthPercentage::new(0.0, 0.05),
                },
                top_right: ComputedCornerRadius {
                    horizontal: ComputedLengthPercentage::new(3.0, 0.0),
                    vertical: ComputedLengthPercentage::new(0.0, 0.06),
                },
                bottom_right: ComputedCornerRadius {
                    horizontal: ComputedLengthPercentage::new(4.0, 0.0),
                    vertical: ComputedLengthPercentage::new(0.0, 0.07),
                },
                bottom_left: ComputedCornerRadius {
                    horizontal: ComputedLengthPercentage::new(5.0, 0.0),
                    vertical: ComputedLengthPercentage::new(0.0, 0.08),
                },
            }),
        }))
    );

    let invalid_path = |path| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Path(path)),
            ),
            None,
            StyleEnvironment::default(),
        )
    };
    for commands in [
        Vec::new(),
        vec![MotionPathCommandValue::LineTo(point(1.0, 1.0))],
        vec![MotionPathCommandValue::QuadraticTo {
            control: point(1.0, 1.0),
            to: point(2.0, 2.0),
        }],
        vec![MotionPathCommandValue::CubicTo {
            control1: point(1.0, 1.0),
            control2: point(2.0, 2.0),
            to: point(3.0, 3.0),
        }],
        vec![MotionPathCommandValue::ArcTo {
            radius_x: number(1.0),
            radius_y: number(1.0),
            x_axis_rotation: number(0.0),
            large_arc: false,
            sweep: true,
            to: point(3.0, 3.0),
        }],
        vec![MotionPathCommandValue::Close],
        vec![MotionPathCommandValue::MoveTo(point(f32::NAN, 0.0))],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::LineTo(point(f32::NAN, 0.0)),
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::QuadraticTo {
                control: point(f32::NAN, 0.0),
                to: point(1.0, 1.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::CubicTo {
                control1: point(f32::NAN, 0.0),
                control2: point(1.0, 1.0),
                to: point(2.0, 2.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: number(f32::NAN),
                radius_y: number(1.0),
                x_axis_rotation: number(0.0),
                large_arc: false,
                sweep: true,
                to: point(2.0, 2.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: number(1.0),
                radius_y: number(f32::NAN),
                x_axis_rotation: number(0.0),
                large_arc: false,
                sweep: true,
                to: point(2.0, 2.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: number(1.0),
                radius_y: number(1.0),
                x_axis_rotation: number(f32::NAN),
                large_arc: false,
                sweep: true,
                to: point(2.0, 2.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: number(1.0),
                radius_y: number(1.0),
                x_axis_rotation: number(0.0),
                large_arc: false,
                sweep: true,
                to: point(f32::NAN, 2.0),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::ArcTo {
                radius_x: number(1.0),
                radius_y: number(1.0),
                x_axis_rotation: number(0.0),
                large_arc: false,
                sweep: true,
                to: point(2.0, f32::NAN),
            },
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(-f32::MAX, 0.0)),
            MotionPathCommandValue::LineTo(point(f32::MAX, 0.0)),
        ],
        vec![
            MotionPathCommandValue::MoveTo(point(1.0, 1.0)),
            MotionPathCommandValue::LineTo(point(1.0, 1.0)),
        ],
    ] {
        assert_eq!(
            invalid_path(commands),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetPath
            ))
        );
    }

    let invalid_declaration = |property, value| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(property, value),
            None,
            StyleEnvironment::default(),
        )
    };
    let invalid_length = px_length(f32::NAN);
    let valid_length = percentage(50.0);
    for path in [
        OffsetPathValue::Circle {
            radius: invalid_length.clone(),
            center_x: valid_length.clone(),
            center_y: valid_length.clone(),
        },
        OffsetPathValue::Circle {
            radius: valid_length.clone(),
            center_x: invalid_length.clone(),
            center_y: valid_length.clone(),
        },
        OffsetPathValue::Circle {
            radius: valid_length.clone(),
            center_x: valid_length.clone(),
            center_y: invalid_length.clone(),
        },
        OffsetPathValue::Ellipse {
            radius_x: invalid_length.clone(),
            radius_y: valid_length.clone(),
            center_x: valid_length.clone(),
            center_y: valid_length.clone(),
        },
        OffsetPathValue::Ellipse {
            radius_x: valid_length.clone(),
            radius_y: invalid_length.clone(),
            center_x: valid_length.clone(),
            center_y: valid_length.clone(),
        },
        OffsetPathValue::Ellipse {
            radius_x: valid_length.clone(),
            radius_y: valid_length.clone(),
            center_x: invalid_length.clone(),
            center_y: valid_length.clone(),
        },
        OffsetPathValue::Ellipse {
            radius_x: valid_length.clone(),
            radius_y: valid_length.clone(),
            center_x: valid_length.clone(),
            center_y: invalid_length.clone(),
        },
    ] {
        assert_eq!(
            invalid_declaration(StyleProperty::OffsetPath, StyleValue::OffsetPath(path),),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetPath
            ))
        );
    }
    for index in 0..4 {
        let mut offsets = std::array::from_fn(|_| valid_length.clone());
        offsets[index] = invalid_length.clone();
        assert_eq!(
            invalid_declaration(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(crate::InsetPathValue {
                    offsets,
                    radii: None,
                },))),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetPath
            ))
        );
    }
    for index in 0..4 {
        let mut radii = std::array::from_fn(|_| BorderRadiusValue {
            horizontal: valid_length.clone(),
            vertical: valid_length.clone(),
        });
        radii[index].horizontal = invalid_length.clone();
        assert_eq!(
            invalid_declaration(
                StyleProperty::OffsetPath,
                StyleValue::OffsetPath(OffsetPathValue::Inset(Box::new(crate::InsetPathValue {
                    offsets: std::array::from_fn(|_| valid_length.clone()),
                    radii: Some(radii),
                },))),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetPath
            ))
        );
    }
    for value in [
        StyleValue::Text("invalid".into()),
        StyleValue::Number(number(f32::NAN)),
        StyleValue::Number(number(-0.1)),
        StyleValue::Number(number(1.1)),
        StyleValue::LengthPercentage(percentage(-1.0)),
        StyleValue::LengthPercentage(percentage(101.0)),
    ] {
        assert_eq!(
            invalid_declaration(StyleProperty::OffsetDistance, value),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetDistance
            ))
        );
    }
    for value in [
        StyleValue::Number(number(0.0)),
        StyleValue::OffsetRotate(OffsetRotateValue::Angle(number(f32::NAN))),
    ] {
        assert_eq!(
            invalid_declaration(StyleProperty::OffsetRotate, value),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::OffsetRotate
            ))
        );
    }
    assert_eq!(
        invalid_declaration(StyleProperty::OffsetPath, StyleValue::Number(number(0.0))),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::OffsetPath
        ))
    );

    assert!(
        invalid_path(vec![
            MotionPathCommandValue::MoveTo(point(0.0, 0.0)),
            MotionPathCommandValue::LineTo(point(10.0, 0.0)),
            MotionPathCommandValue::Close,
        ])
        .is_ok()
    );
}

#[test]
fn logical_borders_resolve_to_physical_edges_and_corners() {
    let specified = |direction| {
        SpecifiedStyle::new()
            .push(StyleProperty::Direction, StyleValue::Direction(direction))
            .push(StyleProperty::BorderInlineStartWidth, px(2.0))
            .push(StyleProperty::BorderInlineEndWidth, px(3.0))
            .push(
                StyleProperty::BorderInlineStartColor,
                StyleValue::Color(ColorValue::Named("start".into())),
            )
            .push(
                StyleProperty::BorderInlineEndColor,
                StyleValue::Color(ColorValue::Named("end".into())),
            )
            .push(
                StyleProperty::BorderInlineStartStyle,
                StyleValue::BorderStyle(BorderStyleValue::Dotted),
            )
            .push(
                StyleProperty::BorderInlineEndStyle,
                StyleValue::BorderStyle(BorderStyleValue::Double),
            )
            .push(StyleProperty::BorderStartStartRadius, px(11.0))
            .push(StyleProperty::BorderStartEndRadius, px(12.0))
            .push(StyleProperty::BorderEndStartRadius, px(13.0))
            .push(StyleProperty::BorderEndEndRadius, px(14.0))
    };

    for (direction, start_is_left) in [
        (crate::DirectionValue::Ltr, true),
        (crate::DirectionValue::Rtl, false),
    ] {
        let resolved =
            crate::resolve_style(&specified(direction), None, StyleEnvironment::default()).unwrap();
        let layout = resolved.computed().layout();
        let paint = resolved.computed().paint();
        let (start_width, end_width) = if start_is_left {
            (layout.border.left.length(), layout.border.right.length())
        } else {
            (layout.border.right.length(), layout.border.left.length())
        };
        assert_eq!((start_width, end_width), (2.0, 3.0));

        let (start_color, end_color, start_style, end_style) = if start_is_left {
            (
                &paint.border_colors.left,
                &paint.border_colors.right,
                paint.border_styles.left,
                paint.border_styles.right,
            )
        } else {
            (
                &paint.border_colors.right,
                &paint.border_colors.left,
                paint.border_styles.right,
                paint.border_styles.left,
            )
        };
        assert_eq!(start_color, &ColorValue::Named("start".into()));
        assert_eq!(end_color, &ColorValue::Named("end".into()));
        assert_eq!(start_style, BorderStyleValue::Dotted);
        assert_eq!(end_style, BorderStyleValue::Double);

        let corners = &paint.border_radii;
        let logical = if start_is_left {
            [
                corners.top_left,
                corners.top_right,
                corners.bottom_left,
                corners.bottom_right,
            ]
        } else {
            [
                corners.top_right,
                corners.top_left,
                corners.bottom_right,
                corners.bottom_left,
            ]
        };
        assert_eq!(
            logical.map(|corner| corner.horizontal.length()),
            [11.0, 12.0, 13.0, 14.0]
        );
    }
}

#[test]
fn logical_and_physical_border_declarations_share_final_write_order() {
    let resolved = crate::resolve_style(
        &SpecifiedStyle::new()
            .push(
                StyleProperty::BorderLeftStyle,
                StyleValue::BorderStyle(BorderStyleValue::Solid),
            )
            .push(StyleProperty::BorderInlineStartWidth, px(2.0))
            .push(StyleProperty::BorderLeftWidth, px(4.0))
            .push(
                StyleProperty::BorderInlineStartColor,
                StyleValue::Color(ColorValue::Named("logical".into())),
            )
            .push(
                StyleProperty::BorderLeftColor,
                StyleValue::Color(ColorValue::Named("physical".into())),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(resolved.computed().layout().border.left.length(), 4.0);
    assert_eq!(
        resolved.computed().paint().border_colors.left,
        ColorValue::Named("physical".into())
    );

    let resolved = crate::resolve_style(
        &SpecifiedStyle::new()
            .push(
                StyleProperty::BorderLeftStyle,
                StyleValue::BorderStyle(BorderStyleValue::Solid),
            )
            .push(StyleProperty::BorderLeftWidth, px(4.0))
            .push(StyleProperty::BorderInlineStartWidth, px(6.0))
            .push(
                StyleProperty::BorderLeftColor,
                StyleValue::Color(ColorValue::Named("physical".into())),
            )
            .push(
                StyleProperty::BorderInlineStartColor,
                StyleValue::Color(ColorValue::Named("logical".into())),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(resolved.computed().layout().border.left.length(), 6.0);
    assert_eq!(
        resolved.computed().paint().border_colors.left,
        ColorValue::Named("logical".into())
    );
}

#[test]
fn background_layer_initial_values_match_css() {
    let resolved =
        crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default()).unwrap();
    assert_eq!(
        resolved.computed().paint().background_layers[0],
        ComputedBackgroundLayerStyle::default()
    );
}

#[test]
fn background_shorthand_resolves_layer_lists_and_empty_initials() {
    let layer = |url: &str, x: f32, size, repeat_x, origin, clip| BackgroundLayerValue {
        image: BackgroundImageValue::Url(url.into()),
        position: BackgroundPositionValue {
            horizontal: percentage(x),
            vertical: px_length(4.0),
        },
        size,
        repeat: BackgroundRepeatValue {
            horizontal: repeat_x,
            vertical: BackgroundRepeatModeValue::NoRepeat,
        },
        origin,
        clip,
        attachment: BackgroundAttachmentValue::Scroll,
    };
    let color = ColorValue::Named("background".into());
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::Background,
            StyleValue::Background(BackgroundValue {
                layers: vec![
                    layer(
                        "front",
                        25.0,
                        BackgroundSizeValue::Cover,
                        BackgroundRepeatModeValue::Space,
                        BackgroundBoxValue::Content,
                        BackgroundBoxValue::Padding,
                    ),
                    layer(
                        "back",
                        75.0,
                        BackgroundSizeValue::Contain,
                        BackgroundRepeatModeValue::Round,
                        BackgroundBoxValue::Border,
                        BackgroundBoxValue::Content,
                    ),
                ],
                color: color.clone().into(),
            }),
        )
        .push(StyleProperty::BackgroundPositionY, px(9.0));
    let resolved = crate::resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
    let paint = resolved.computed().paint();
    assert_eq!(paint.background_color, color);
    assert_eq!(
        paint.background_images,
        vec![
            ComputedBackgroundImage::Url("front".into()),
            ComputedBackgroundImage::Url("back".into()),
        ]
    );
    assert_eq!(paint.background_layers.len(), 2);
    assert_eq!(
        paint.background_layers[0].position.horizontal.fraction(),
        0.25
    );
    assert_eq!(
        paint.background_layers[1].position.horizontal.fraction(),
        0.75
    );
    assert!(
        paint
            .background_layers
            .iter()
            .all(|layer| layer.position.vertical.length() == 9.0)
    );
    assert_eq!(
        paint.background_layers[0].size,
        ComputedBackgroundSize::Cover
    );
    assert_eq!(
        paint.background_layers[1].size,
        ComputedBackgroundSize::Contain
    );

    let empty = crate::resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Background,
            StyleValue::Background(BackgroundValue {
                layers: Vec::new(),
                color: ColorValue::Named("empty".into()).into(),
            }),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert!(empty.computed().paint().background_images.is_empty());
    assert_eq!(empty.computed().paint().background_layers.len(), 1);

    let mut invalid_position = layer(
        "invalid-position",
        0.0,
        BackgroundSizeValue::Auto,
        BackgroundRepeatModeValue::Repeat,
        BackgroundBoxValue::Padding,
        BackgroundBoxValue::Border,
    );
    invalid_position.position.horizontal = px_length(f32::NAN);
    let mut invalid_size = layer(
        "invalid-size",
        0.0,
        BackgroundSizeValue::Auto,
        BackgroundRepeatModeValue::Repeat,
        BackgroundBoxValue::Padding,
        BackgroundBoxValue::Border,
    );
    invalid_size.size = BackgroundSizeValue::Explicit {
        width: Some(px_length(-1.0)),
        height: None,
    };
    let invalid_image = layer(
        "  ",
        0.0,
        BackgroundSizeValue::Auto,
        BackgroundRepeatModeValue::Repeat,
        BackgroundBoxValue::Padding,
        BackgroundBoxValue::Border,
    );
    for layer in [invalid_position, invalid_size, invalid_image] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::Background,
                    StyleValue::Background(BackgroundValue {
                        layers: vec![layer],
                        color: ColorValue::Named("invalid".into()).into(),
                    }),
                ),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::Background
            ))
        );
    }
}

#[test]
fn background_gradients_resolve_all_shapes_and_reject_invalid_values() {
    let stop = |name: &str, position| GradientStopValue {
        color: ColorValue::Named(name.into()).into(),
        position,
    };
    let stops = || {
        vec![
            stop("red", None),
            stop("gold", None),
            stop("blue", Some(percentage(100.0))),
        ]
    };
    let resolve = |image| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::BackgroundImage,
                StyleValue::BackgroundImages(vec![image]),
            ),
            None,
            StyleEnvironment::default(),
        )
    };

    assert_eq!(
        resolve(BackgroundImageValue::None)
            .unwrap()
            .computed()
            .paint()
            .background_images,
        vec![ComputedBackgroundImage::None]
    );
    let linear = resolve(BackgroundImageValue::Gradient(GradientValue::Linear {
        angle_degrees: number(90.0).into(),
        stops: stops(),
    }))
    .unwrap();
    assert!(matches!(
        &linear.computed().paint().background_images[0],
        ComputedBackgroundImage::Gradient(ComputedGradient::Linear {
            angle_degrees,
            stops,
        }) if angle_degrees.get() == 90.0
            && stops[0].position.unwrap().fraction() == 0.0
            && stops[1].position.unwrap().fraction() == 0.5
            && stops[2].position.unwrap().fraction() == 1.0
    ));
    let implicit_endpoints = resolve(BackgroundImageValue::Gradient(GradientValue::Linear {
        angle_degrees: number(0.0).into(),
        stops: vec![stop("start", None), stop("end", None)],
    }))
    .unwrap();
    assert!(matches!(
        &implicit_endpoints.computed().paint().background_images[0],
        ComputedBackgroundImage::Gradient(ComputedGradient::Linear { stops, .. })
            if stops[0].position.unwrap().fraction() == 0.0
                && stops[1].position.unwrap().fraction() == 1.0
    ));

    for (shape, circle, explicit) in [
        (RadialGradientValue::Circle, true, false),
        (RadialGradientValue::Ellipse, false, false),
        (
            RadialGradientValue::CircleSized(px_length(20.0)),
            true,
            true,
        ),
        (
            RadialGradientValue::EllipseSized(px_length(30.0), percentage(40.0)),
            false,
            true,
        ),
    ] {
        let resolved = resolve(BackgroundImageValue::Gradient(GradientValue::Radial {
            shape,
            stops: stops(),
        }))
        .unwrap();
        assert!(matches!(
            &resolved.computed().paint().background_images[0],
            ComputedBackgroundImage::Gradient(ComputedGradient::Radial {
                circle: actual_circle,
                radii,
                ..
            }) if *actual_circle == circle && radii.is_some() == explicit
        ));
    }

    let conic = resolve(BackgroundImageValue::Gradient(GradientValue::Conic {
        from_degrees: number(45.0).into(),
        center: BackgroundPositionValue {
            horizontal: percentage(25.0),
            vertical: percentage(75.0),
        },
        stops: stops(),
    }))
    .unwrap();
    assert!(matches!(
        &conic.computed().paint().background_images[0],
        ComputedBackgroundImage::Gradient(ComputedGradient::Conic {
            from_degrees,
            center,
            ..
        }) if from_degrees.get() == 45.0
            && center.horizontal.fraction() == 0.25
            && center.vertical.fraction() == 0.75
    ));

    let invalid_color = GradientStopValue {
        color: ColorValue::Rgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: number(f32::NAN),
        }
        .into(),
        position: None,
    };
    let invalid = [
        BackgroundImageValue::Url("  ".into()),
        BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(f32::NAN).into(),
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(0.0).into(),
            stops: vec![stop("only", None)],
        }),
        BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(0.0).into(),
            stops: vec![
                stop("bad-position", Some(px_length(f32::NAN))),
                stop("end", None),
            ],
        }),
        BackgroundImageValue::Gradient(GradientValue::Linear {
            angle_degrees: number(0.0).into(),
            stops: vec![invalid_color, stop("end", None)],
        }),
        BackgroundImageValue::Gradient(GradientValue::Radial {
            shape: RadialGradientValue::CircleSized(px_length(f32::NAN)),
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Radial {
            shape: RadialGradientValue::EllipseSized(px_length(1.0), px_length(f32::NAN)),
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Radial {
            shape: RadialGradientValue::EllipseSized(px_length(f32::NAN), px_length(1.0)),
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Radial {
            shape: RadialGradientValue::Circle,
            stops: vec![stop("only", None)],
        }),
        BackgroundImageValue::Gradient(GradientValue::Conic {
            from_degrees: number(f32::NAN).into(),
            center: BackgroundPositionValue {
                horizontal: percentage(50.0),
                vertical: percentage(50.0),
            },
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Conic {
            from_degrees: number(0.0).into(),
            center: BackgroundPositionValue {
                horizontal: px_length(f32::NAN),
                vertical: percentage(50.0),
            },
            stops: stops(),
        }),
        BackgroundImageValue::Gradient(GradientValue::Conic {
            from_degrees: number(0.0).into(),
            center: BackgroundPositionValue {
                horizontal: percentage(50.0),
                vertical: percentage(50.0),
            },
            stops: vec![stop("only", None)],
        }),
    ];
    for image in invalid {
        assert_eq!(
            resolve(image),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BackgroundImage
            ))
        );
    }
}

#[test]
fn paint_values_resolve_without_host_types() {
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named("current".into())),
        )
        .push(
            StyleProperty::BackgroundColor,
            StyleValue::Color(ColorValue::Rgba {
                red: 1,
                green: 2,
                blue: 3,
                alpha: number(0.5),
            }),
        )
        .push(
            StyleProperty::BackgroundImage,
            StyleValue::BackgroundImages(vec![BackgroundImageValue::Url(
                "https://example.com/image.png".into(),
            )]),
        )
        .push(
            StyleProperty::BackgroundPosition,
            StyleValue::BackgroundPosition(BackgroundPositionValue {
                horizontal: percentage(25.0),
                vertical: px_length(10.0),
            }),
        )
        .push(StyleProperty::BackgroundPositionX, px(5.0))
        .push(
            StyleProperty::BackgroundSize,
            StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                width: Some(percentage(50.0)),
                height: Some(px_length(20.0)),
            }),
        )
        .push(
            StyleProperty::BackgroundRepeat,
            StyleValue::BackgroundRepeat(BackgroundRepeatValue {
                horizontal: BackgroundRepeatModeValue::Space,
                vertical: BackgroundRepeatModeValue::Round,
            }),
        )
        .push(
            StyleProperty::BackgroundOrigin,
            StyleValue::BackgroundBox(BackgroundBoxValue::Content),
        )
        .push(
            StyleProperty::BackgroundClip,
            StyleValue::BackgroundBox(BackgroundBoxValue::Padding),
        )
        .push(
            StyleProperty::BackgroundAttachment,
            StyleValue::BackgroundAttachment(BackgroundAttachmentValue::Scroll),
        )
        .push(
            StyleProperty::BorderTopStyle,
            StyleValue::BorderStyle(BorderStyleValue::Solid),
        )
        .push(
            StyleProperty::BorderTopColor,
            StyleValue::Color(ColorValue::Named("top".into())),
        )
        .push(
            StyleProperty::BorderRightColor,
            StyleValue::Color(ColorValue::Named("right".into())),
        )
        .push(
            StyleProperty::BorderBottomColor,
            StyleValue::Color(ColorValue::Named("bottom".into())),
        )
        .push(
            StyleProperty::BorderLeftColor,
            StyleValue::Color(ColorValue::Named("left".into())),
        )
        .push(
            StyleProperty::BorderRightStyle,
            StyleValue::BorderStyle(BorderStyleValue::Dashed),
        )
        .push(
            StyleProperty::BorderBottomStyle,
            StyleValue::BorderStyle(BorderStyleValue::Dotted),
        )
        .push(
            StyleProperty::BorderLeftStyle,
            StyleValue::BorderStyle(BorderStyleValue::Double),
        )
        .push(StyleProperty::BorderTopLeftRadius, px(8.0))
        .push(
            StyleProperty::BorderTopRightRadius,
            StyleValue::BorderRadius(BorderRadiusValue {
                horizontal: px_length(9.0),
                vertical: px_length(4.0),
            }),
        )
        .push(StyleProperty::BorderBottomRightRadius, px(10.0))
        .push(StyleProperty::BorderBottomLeftRadius, px(11.0))
        .push(
            StyleProperty::ImageRendering,
            StyleValue::ImageRendering(ImageRenderingValue::Pixelated),
        )
        .push(StyleProperty::Opacity, StyleValue::Number(number(2.0)))
        .push(
            StyleProperty::OverflowX,
            StyleValue::Overflow(OverflowValue::Hidden),
        )
        .push(
            StyleProperty::OverflowY,
            StyleValue::Overflow(OverflowValue::Hidden),
        )
        .push(
            StyleProperty::Visibility,
            StyleValue::Visibility(VisibilityValue::Hidden),
        )
        .push(StyleProperty::ZIndex, StyleValue::Integer(-3));
    let resolved = crate::resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
    let paint = resolved.computed().paint();
    assert_eq!(
        paint.background_color,
        ColorValue::Rgba {
            red: 1,
            green: 2,
            blue: 3,
            alpha: number(0.5),
        }
    );
    assert_eq!(
        paint.background_images,
        vec![ComputedBackgroundImage::Url(
            "https://example.com/image.png".into()
        )]
    );
    assert_eq!(paint.background_layers[0].position.horizontal.length(), 5.0);
    assert_eq!(
        paint.background_layers[0].position.horizontal.fraction(),
        0.0
    );
    assert_eq!(paint.background_layers[0].position.vertical.length(), 10.0);
    assert_eq!(
        paint.background_layers[0].size,
        ComputedBackgroundSize::Explicit {
            width: Some(ComputedLengthPercentage::new(0.0, 0.5)),
            height: Some(ComputedLengthPercentage::new(20.0, 0.0)),
        }
    );
    assert_eq!(
        paint.background_layers[0].repeat_x,
        BackgroundRepeatModeValue::Space
    );
    assert_eq!(
        paint.background_layers[0].repeat_y,
        BackgroundRepeatModeValue::Round
    );
    assert_eq!(
        paint.background_layers[0].origin,
        BackgroundBoxValue::Content
    );
    assert_eq!(paint.background_layers[0].clip, BackgroundBoxValue::Padding);
    assert_eq!(
        paint.background_layers[0].attachment,
        BackgroundAttachmentValue::Scroll
    );
    assert_eq!(paint.border_colors.top, ColorValue::Named("top".into()));
    assert_eq!(paint.border_colors.right, ColorValue::Named("right".into()));
    assert_eq!(
        paint.border_colors.bottom,
        ColorValue::Named("bottom".into())
    );
    assert_eq!(paint.border_colors.left, ColorValue::Named("left".into()));
    assert_eq!(paint.border_styles.top, BorderStyleValue::Solid);
    assert_eq!(paint.border_styles.right, BorderStyleValue::Dashed);
    assert_eq!(paint.border_styles.bottom, BorderStyleValue::Dotted);
    assert_eq!(paint.border_styles.left, BorderStyleValue::Double);
    assert_eq!(paint.border_radii.top_left.horizontal.length(), 8.0);
    assert_eq!(paint.border_radii.top_left.vertical.length(), 8.0);
    assert_eq!(paint.border_radii.top_right.horizontal.length(), 9.0);
    assert_eq!(paint.border_radii.top_right.vertical.length(), 4.0);
    assert_eq!(paint.border_radii.bottom_right.horizontal.length(), 10.0);
    assert_eq!(paint.border_radii.bottom_left.horizontal.length(), 11.0);
    assert_eq!(paint.image_rendering, ImageRenderingValue::Pixelated);
    assert_eq!(paint.opacity.get(), 1.0);
    assert_eq!(paint.overflow_x, OverflowValue::Hidden);
    assert_eq!(paint.overflow_y, OverflowValue::Hidden);
    assert_eq!(paint.visibility, VisibilityValue::Hidden);
    assert_eq!(paint.z_index, -3);

    assert!(paint.changes_from(paint).is_empty());
    let mut changed = paint.clone();
    changed.opacity = number(0.5);
    assert_eq!(changed.changes_from(paint), crate::PropertyImpactSet::PAINT);
}

#[test]
fn background_geometry_resolves_keywords_auto_axes_and_axis_errors() {
    let resolve_size = |size| {
        crate::resolve_style(
            &SpecifiedStyle::new().push(
                StyleProperty::BackgroundSize,
                StyleValue::BackgroundSize(size),
            ),
            None,
            StyleEnvironment::default(),
        )
    };
    for (specified, computed) in [
        (BackgroundSizeValue::Auto, ComputedBackgroundSize::Auto),
        (BackgroundSizeValue::Cover, ComputedBackgroundSize::Cover),
        (
            BackgroundSizeValue::Contain,
            ComputedBackgroundSize::Contain,
        ),
        (
            BackgroundSizeValue::Explicit {
                width: None,
                height: None,
            },
            ComputedBackgroundSize::Auto,
        ),
        (
            BackgroundSizeValue::Explicit {
                width: Some(px_length(12.0)),
                height: None,
            },
            ComputedBackgroundSize::Explicit {
                width: Some(ComputedLengthPercentage::new(12.0, 0.0)),
                height: None,
            },
        ),
        (
            BackgroundSizeValue::Explicit {
                width: None,
                height: Some(percentage(25.0)),
            },
            ComputedBackgroundSize::Explicit {
                width: None,
                height: Some(ComputedLengthPercentage::new(0.0, 0.25)),
            },
        ),
    ] {
        assert_eq!(
            resolve_size(specified)
                .unwrap()
                .computed()
                .paint()
                .background_layers[0]
                .size,
            computed
        );
    }

    for position in [
        BackgroundPositionValue {
            horizontal: px_length(f32::NAN),
            vertical: px_length(0.0),
        },
        BackgroundPositionValue {
            horizontal: px_length(0.0),
            vertical: px_length(f32::NAN),
        },
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(
                    StyleProperty::BackgroundPosition,
                    StyleValue::BackgroundPosition(position),
                ),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BackgroundPosition
            ))
        );
    }

    for size in [
        BackgroundSizeValue::Explicit {
            width: Some(px_length(f32::NAN)),
            height: None,
        },
        BackgroundSizeValue::Explicit {
            width: None,
            height: Some(px_length(f32::NAN)),
        },
        BackgroundSizeValue::Explicit {
            width: None,
            height: Some(px_length(-1.0)),
        },
    ] {
        assert_eq!(
            resolve_size(size),
            Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::BackgroundSize
            ))
        );
    }
}

#[test]
fn invalid_paint_values_are_diagnostic() {
    for property in [
        StyleProperty::Background,
        StyleProperty::BackgroundColor,
        StyleProperty::BackgroundImage,
        StyleProperty::BackgroundRepeat,
        StyleProperty::BackgroundPosition,
        StyleProperty::BackgroundPositionX,
        StyleProperty::BackgroundPositionY,
        StyleProperty::BackgroundSize,
        StyleProperty::BackgroundOrigin,
        StyleProperty::BackgroundClip,
        StyleProperty::BackgroundAttachment,
        StyleProperty::ImageRendering,
        StyleProperty::BorderTopColor,
        StyleProperty::BorderRightColor,
        StyleProperty::BorderBottomColor,
        StyleProperty::BorderLeftColor,
        StyleProperty::BorderInlineStartColor,
        StyleProperty::BorderInlineEndColor,
        StyleProperty::BorderTopStyle,
        StyleProperty::BorderRightStyle,
        StyleProperty::BorderBottomStyle,
        StyleProperty::BorderLeftStyle,
        StyleProperty::BorderInlineStartStyle,
        StyleProperty::BorderInlineEndStyle,
        StyleProperty::BorderTopLeftRadius,
        StyleProperty::BorderTopRightRadius,
        StyleProperty::BorderBottomRightRadius,
        StyleProperty::BorderBottomLeftRadius,
        StyleProperty::BorderStartStartRadius,
        StyleProperty::BorderStartEndRadius,
        StyleProperty::BorderEndStartRadius,
        StyleProperty::BorderEndEndRadius,
        StyleProperty::Opacity,
        StyleProperty::Visibility,
        StyleProperty::ZIndex,
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
    let inherited = crate::resolve_style(&SpecifiedStyle::new(), None, StyleEnvironment::default())
        .unwrap()
        .computed()
        .inherited_text()
        .clone();
    for property in [StyleProperty::OverflowX, StyleProperty::OverflowY] {
        assert_eq!(
            resolve_paint_style(
                &SpecifiedStyle::new().push(property, StyleValue::Bool(true)),
                &inherited,
                DirectionValue::Ltr,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
    for property in [
        StyleProperty::BorderInlineStartColor,
        StyleProperty::BorderInlineEndColor,
        StyleProperty::BorderInlineStartStyle,
        StyleProperty::BorderInlineEndStyle,
        StyleProperty::BorderStartStartRadius,
        StyleProperty::BorderStartEndRadius,
        StyleProperty::BorderEndStartRadius,
        StyleProperty::BorderEndEndRadius,
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new()
                    .push(
                        StyleProperty::Direction,
                        StyleValue::Direction(DirectionValue::Rtl),
                    )
                    .push(property, StyleValue::Bool(true)),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
    assert_eq!(
        crate::resolve_style(
            &SpecifiedStyle::new().push(StyleProperty::BorderTopLeftRadius, px(-1.0)),
            None,
            StyleEnvironment::default(),
        ),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::BorderTopLeftRadius
        ))
    );
    for (property, value) in [
        (
            StyleProperty::BackgroundColor,
            StyleValue::Color(ColorValue::Named(String::new())),
        ),
        (
            StyleProperty::BackgroundSize,
            StyleValue::BackgroundSize(BackgroundSizeValue::Explicit {
                width: Some(px_length(-1.0)),
                height: Some(px_length(1.0)),
            }),
        ),
        (StyleProperty::Opacity, StyleValue::Number(number(f32::NAN))),
        (StyleProperty::ZIndex, StyleValue::Integer(i64::MAX)),
        (StyleProperty::BorderTopLeftRadius, px(f32::NAN)),
        (
            StyleProperty::BorderTopRightRadius,
            StyleValue::BorderRadius(BorderRadiusValue {
                horizontal: px_length(1.0),
                vertical: px_length(f32::NAN),
            }),
        ),
    ] {
        assert_eq!(
            crate::resolve_style(
                &SpecifiedStyle::new().push(property, value),
                None,
                StyleEnvironment::default(),
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
}
