use super::*;

fn number(value: f32) -> StyleNumber {
    StyleNumber::new(value)
}

fn px(value: f32) -> LengthValue {
    LengthValue::Dimension {
        value: number(value),
        unit: LengthUnit::Px,
    }
}

fn length(value: f32, unit: LengthUnit) -> LengthPercentageValue {
    LengthPercentageValue::Length(LengthValue::Dimension {
        value: number(value),
        unit,
    })
}

fn declaration(property: StyleProperty, value: StyleValue) -> SpecifiedStyle {
    SpecifiedStyle::new().push(property, value)
}

fn component_variable<T>(name: &CustomPropertyName) -> ComponentValue<T> {
    ComponentValue::Variable(CustomPropertyReference::new(name.clone()))
}

fn inherited(style: &ResolvedNodeStyle) -> &InheritedStyle {
    assert_eq!(
        style.computed().inherited_text(),
        style.inherited_for_children()
    );
    style.inherited_for_children()
}

#[test]
fn root_uses_documented_initial_text_context() {
    let environment = StyleEnvironment::default();
    assert_eq!(environment.viewport_width(), 0.0);
    assert_eq!(environment.viewport_height(), 0.0);
    assert_eq!(environment.scale_factor(), 1.0);
    assert_eq!(environment.root_font_size(), 14.0);

    let resolved = resolve_text_style(&SpecifiedStyle::new(), None, environment).unwrap();
    assert_eq!(
        resolved.computed().layout(),
        &ComputedLayoutStyle::default()
    );
    let text = inherited(&resolved);
    assert_eq!(text.font_family(), &FontFamilyValue::System);
    assert_eq!(text.font_size(), 14.0);
    assert_eq!(text.font_weight(), FontWeightValue::NORMAL);
    assert_eq!(text.font_style(), FontStyleValue::Normal);
    assert_eq!(text.line_height(), ComputedLineHeight::Normal);
    assert_eq!(text.letter_spacing(), 0.0);
    assert_eq!(
        text.color(),
        &ColorValue::Rgba {
            red: 0,
            green: 0,
            blue: 0,
            alpha: number(1.0),
        }
    );
}

#[test]
fn cursor_and_pointer_events_resolve_and_inherit_as_typed_input_values() {
    let environment = StyleEnvironment::default();
    let parent = resolve_style(
        &SpecifiedStyle::new()
            .push(StyleProperty::Cursor, StyleValue::Cursor(CursorValue::Grab))
            .push(
                StyleProperty::PointerEvents,
                StyleValue::PointerEvents(PointerEventsValue::None),
            ),
        None,
        environment,
    )
    .unwrap();
    assert_eq!(parent.computed().cursor(), CursorValue::Grab);
    assert_eq!(parent.computed().pointer_events(), PointerEventsValue::None);

    let child = resolve_style(
        &SpecifiedStyle::new(),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(child.computed().cursor(), CursorValue::Grab);
    assert_eq!(child.computed().pointer_events(), PointerEventsValue::None);

    let reset = resolve_style(
        &SpecifiedStyle::new()
            .push(StyleProperty::Cursor, StyleValue::Cursor(CursorValue::Auto))
            .push(
                StyleProperty::PointerEvents,
                StyleValue::PointerEvents(PointerEventsValue::Auto),
            ),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(reset.computed().cursor(), CursorValue::Auto);
    assert_eq!(reset.computed().pointer_events(), PointerEventsValue::Auto);

    for property in [StyleProperty::Cursor, StyleProperty::PointerEvents] {
        assert_eq!(
            resolve_style(
                &SpecifiedStyle::new().push(property, StyleValue::Number(number(1.0))),
                None,
                environment,
            ),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
}

#[test]
fn extended_font_settings_are_canonical_inherited_and_validated() {
    let tag = |value| crate::OpenTypeTagValue::new(value).unwrap();
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::FontFeatureSettings,
            StyleValue::FontFeatures(vec![
                FontFeatureValue {
                    tag: tag(*b"liga"),
                    value: 1,
                },
                FontFeatureValue {
                    tag: tag(*b"kern"),
                    value: 1,
                },
                FontFeatureValue {
                    tag: tag(*b"kern"),
                    value: 0,
                },
            ]),
        )
        .push(
            StyleProperty::FontVariationSettings,
            StyleValue::FontVariations(vec![
                FontVariationValue {
                    tag: tag(*b"wght"),
                    value: number(400.0),
                },
                FontVariationValue {
                    tag: tag(*b"wdth"),
                    value: number(90.0),
                },
                FontVariationValue {
                    tag: tag(*b"wght"),
                    value: number(650.0),
                },
            ]),
        )
        .push(
            StyleProperty::FontOpticalSizing,
            StyleValue::FontOpticalSizing(FontOpticalSizingValue::Auto),
        );
    let parent = resolve_text_style(&specified, None, StyleEnvironment::default()).unwrap();
    let inherited = parent.inherited_for_children();
    assert_eq!(
        inherited.font_features(),
        [
            FontFeatureValue {
                tag: tag(*b"kern"),
                value: 0,
            },
            FontFeatureValue {
                tag: tag(*b"liga"),
                value: 1,
            },
        ]
    );
    assert_eq!(inherited.font_variations()[0].tag, tag(*b"wdth"));
    assert_eq!(inherited.font_variations()[0].value.get(), 90.0);
    assert_eq!(inherited.font_variations()[1].tag, tag(*b"wght"));
    assert_eq!(inherited.font_variations()[1].value.get(), 650.0);
    assert_eq!(
        inherited.font_optical_sizing(),
        FontOpticalSizingValue::Auto
    );

    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(inherited),
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(child.inherited_for_children(), inherited);

    let invalid = declaration(
        StyleProperty::FontVariationSettings,
        StyleValue::FontVariations(vec![FontVariationValue {
            tag: tag(*b"wght"),
            value: number(f32::NAN),
        }]),
    );
    assert_eq!(
        resolve_text_style(&invalid, None, StyleEnvironment::default()).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::FontVariationSettings)
    );
}

#[test]
fn layout_resolution_errors_propagate_from_the_combined_resolver() {
    let error = resolve_style(
        &declaration(StyleProperty::Width, StyleValue::Bool(true)),
        None,
        StyleEnvironment::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        StyleResolutionError::InvalidPropertyValue(StyleProperty::Width)
    );
}

#[test]
fn child_inherits_parent_and_explicit_values_stop_inheritance() {
    let environment = StyleEnvironment::new(750.0, 800.0, 2.0, 16.0);
    let parent_specified = SpecifiedStyle::new()
        .push(
            StyleProperty::FontFamily,
            StyleValue::FontFamily(FontFamilyValue::Named("Inter".into())),
        )
        .push(
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(length(20.0, LengthUnit::Px)),
        )
        .push(
            StyleProperty::FontWeight,
            StyleValue::FontWeight(FontWeightValue::BOLD),
        )
        .push(
            StyleProperty::FontStyle,
            StyleValue::FontStyle(FontStyleValue::Italic),
        )
        .push(
            StyleProperty::LineHeight,
            StyleValue::LineHeight(LineHeightValue::Number(number(1.5))),
        )
        .push(StyleProperty::LetterSpacing, StyleValue::Length(px(2.0)))
        .push(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named("red".into())),
        );
    let parent = resolve_text_style(&parent_specified, None, environment).unwrap();
    let child = resolve_text_style(
        &declaration(
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(LengthPercentageValue::Percentage(number(50.0))),
        ),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    let child = inherited(&child);
    assert_eq!(child.font_family(), &FontFamilyValue::Named("Inter".into()));
    assert_eq!(child.font_size(), 10.0);
    assert_eq!(child.font_weight(), FontWeightValue::BOLD);
    assert_eq!(child.font_style(), FontStyleValue::Italic);
    assert_eq!(
        child.line_height(),
        ComputedLineHeight::LogicalPixels(number(30.0))
    );
    assert_eq!(child.letter_spacing(), 2.0);
    assert_eq!(child.color(), &ColorValue::Named("red".into()));
}

#[test]
fn declaration_order_and_unrelated_properties_do_not_change_resolution() {
    let specified = SpecifiedStyle::new()
        .push(StyleProperty::Opacity, StyleValue::Number(number(0.5)))
        .push(
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(length(10.0, LengthUnit::Px)),
        )
        .push(
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(length(12.0, LengthUnit::Px)),
        );
    assert_eq!(
        inherited(&resolve_text_style(&specified, None, StyleEnvironment::default()).unwrap())
            .font_size(),
        12.0
    );
}

#[test]
fn relative_units_use_the_correct_environment_basis() {
    let environment = StyleEnvironment::new(750.0, 400.0, 2.0, 10.0);
    let cases = [
        (LengthValue::Zero, 0.0),
        (px(3.0), 3.0),
        (
            LengthValue::Dimension {
                value: number(2.0),
                unit: LengthUnit::Em,
            },
            12.0,
        ),
        (
            LengthValue::Dimension {
                value: number(2.0),
                unit: LengthUnit::Rem,
            },
            20.0,
        ),
        (
            LengthValue::Dimension {
                value: number(2.0),
                unit: LengthUnit::Vh,
            },
            8.0,
        ),
        (
            LengthValue::Dimension {
                value: number(2.0),
                unit: LengthUnit::Vw,
            },
            15.0,
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(
            resolve_length(value, 6.0, environment, StyleProperty::LetterSpacing).unwrap(),
            expected
        );
    }
}

#[test]
fn line_height_resolves_normal_number_and_relative_length() {
    let environment = StyleEnvironment::default();
    let number_style = declaration(
        StyleProperty::LineHeight,
        StyleValue::LineHeight(LineHeightValue::Number(number(2.0))),
    );
    assert_eq!(
        inherited(&resolve_text_style(&number_style, None, environment).unwrap()).line_height(),
        ComputedLineHeight::LogicalPixels(number(28.0))
    );
    let length_style = declaration(
        StyleProperty::LineHeight,
        StyleValue::LineHeight(LineHeightValue::LengthPercentage(
            LengthPercentageValue::Percentage(number(150.0)),
        )),
    );
    assert_eq!(
        inherited(&resolve_text_style(&length_style, None, environment).unwrap()).line_height(),
        ComputedLineHeight::LogicalPixels(number(21.0))
    );
    let normal_style = declaration(
        StyleProperty::LineHeight,
        StyleValue::LineHeight(LineHeightValue::Normal),
    );
    let parent = resolve_text_style(&number_style, None, environment).unwrap();
    let normal = resolve_text_style(
        &normal_style,
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(inherited(&normal).line_height(), ComputedLineHeight::Normal);
}

#[test]
fn calc_supports_valid_dimension_arithmetic() {
    let environment = StyleEnvironment::default();
    let leaf = || CalcExpression::Value(Box::new(LengthPercentageValue::Length(px(10.0))));
    let scalar = |value| CalcExpression::Number(number(value));
    let cases = [
        (
            CalcExpression::Add(Box::new(leaf()), Box::new(leaf())),
            20.0,
        ),
        (CalcExpression::Sub(Box::new(leaf()), Box::new(leaf())), 0.0),
        (
            CalcExpression::Mul(Box::new(scalar(2.0)), Box::new(leaf())),
            20.0,
        ),
        (
            CalcExpression::Mul(Box::new(leaf()), Box::new(scalar(3.0))),
            30.0,
        ),
        (
            CalcExpression::Div(Box::new(leaf()), Box::new(scalar(2.0))),
            5.0,
        ),
    ];
    for (expression, expected) in cases {
        assert_eq!(
            resolve_length_percentage(
                &LengthPercentageValue::Calc(Box::new(expression)),
                14.0,
                14.0,
                environment,
                StyleProperty::FontSize,
            )
            .unwrap(),
            expected
        );
    }
}

#[test]
fn calc_evaluates_scalar_branches_and_rejects_invalid_dimensions() {
    let environment = StyleEnvironment::default();
    let scalar = |value| CalcExpression::Number(number(value));
    let length = || CalcExpression::Value(Box::new(LengthPercentageValue::Length(px(4.0))));
    for expression in [
        CalcExpression::Add(Box::new(scalar(1.0)), Box::new(scalar(2.0))),
        CalcExpression::Sub(Box::new(scalar(3.0)), Box::new(scalar(1.0))),
        CalcExpression::Mul(Box::new(scalar(2.0)), Box::new(scalar(3.0))),
        CalcExpression::Div(Box::new(scalar(6.0)), Box::new(scalar(2.0))),
        CalcExpression::Div(Box::new(length()), Box::new(length())),
    ] {
        evaluate_calc(
            &expression,
            10.0,
            10.0,
            environment,
            StyleProperty::FontSize,
        )
        .unwrap();
    }
    for expression in [
        CalcExpression::Add(Box::new(scalar(1.0)), Box::new(length())),
        CalcExpression::Sub(Box::new(length()), Box::new(scalar(1.0))),
        CalcExpression::Mul(Box::new(length()), Box::new(length())),
        CalcExpression::Div(Box::new(scalar(1.0)), Box::new(length())),
        CalcExpression::Div(Box::new(length()), Box::new(scalar(0.0))),
        CalcExpression::Div(
            Box::new(scalar(1.0)),
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Length(LengthValue::Zero),
            ))),
        ),
    ] {
        assert_eq!(
            evaluate_calc(
                &expression,
                10.0,
                10.0,
                environment,
                StyleProperty::FontSize
            )
            .unwrap_err(),
            StyleResolutionError::InvalidCalculation(StyleProperty::FontSize)
        );
    }
    let scalar_result = LengthPercentageValue::Calc(Box::new(scalar(2.0)));
    assert_eq!(
        resolve_length_percentage(
            &scalar_result,
            10.0,
            10.0,
            environment,
            StyleProperty::FontSize
        )
        .unwrap_err(),
        StyleResolutionError::InvalidCalculation(StyleProperty::FontSize)
    );
}

#[test]
fn invalid_environment_values_are_rejected() {
    for environment in [
        StyleEnvironment::new(f32::NAN, 0.0, 1.0, 14.0),
        StyleEnvironment::new(-1.0, 0.0, 1.0, 14.0),
        StyleEnvironment::new(0.0, f32::INFINITY, 1.0, 14.0),
        StyleEnvironment::new(0.0, -1.0, 1.0, 14.0),
        StyleEnvironment::new(0.0, 0.0, f32::NAN, 14.0),
        StyleEnvironment::new(0.0, 0.0, 0.0, 14.0),
        StyleEnvironment::new(0.0, 0.0, 1.0, f32::INFINITY),
        StyleEnvironment::new(0.0, 0.0, 1.0, -1.0),
    ] {
        assert_eq!(
            resolve_text_style(&SpecifiedStyle::new(), None, environment).unwrap_err(),
            StyleResolutionError::InvalidEnvironment
        );
    }
}

#[test]
fn wrong_semantic_variants_are_reported_per_property() {
    for property in [
        StyleProperty::FontFamily,
        StyleProperty::FontFeatureSettings,
        StyleProperty::FontVariationSettings,
        StyleProperty::FontOpticalSizing,
        StyleProperty::FontSize,
        StyleProperty::FontWeight,
        StyleProperty::FontStyle,
        StyleProperty::LineHeight,
        StyleProperty::LetterSpacing,
        StyleProperty::Color,
        StyleProperty::TextAlign,
        StyleProperty::TextIndent,
        StyleProperty::WhiteSpace,
        StyleProperty::WordBreak,
        StyleProperty::TextOverflow,
        StyleProperty::TextDecoration,
        StyleProperty::TextShadow,
    ] {
        let error = resolve_text_style(
            &declaration(property, StyleValue::Bool(true)),
            None,
            StyleEnvironment::default(),
        )
        .unwrap_err();
        assert_eq!(error, StyleResolutionError::InvalidPropertyValue(property));
        assert_eq!(
            error.to_string(),
            format!("invalid value for `{}`", property.css_name())
        );
    }
    assert_eq!(
        StyleResolutionError::InvalidEnvironment.to_string(),
        "invalid style environment"
    );
    assert_eq!(
        StyleResolutionError::InvalidCalculation(StyleProperty::FontSize).to_string(),
        "invalid calculation for `font-size`"
    );
}

#[test]
fn text_alignment_resolves_and_inherits() {
    let parent = resolve_text_style(
        &declaration(
            StyleProperty::TextAlign,
            StyleValue::TextAlign(TextAlignValue::Center),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        parent.inherited_for_children().text_align(),
        TextAlignValue::Center
    );
    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(parent.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        child.inherited_for_children().text_align(),
        TextAlignValue::Center
    );
}

#[test]
fn direction_resolves_and_inherits_into_layout_and_text_context() {
    let parent = resolve_text_style(
        &declaration(
            StyleProperty::Direction,
            StyleValue::Direction(DirectionValue::Rtl),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(parent.computed().layout().direction, DirectionValue::Rtl);
    assert_eq!(
        parent.inherited_for_children().direction(),
        DirectionValue::Rtl
    );

    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(parent.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(child.computed().layout().direction, DirectionValue::Rtl);
    assert_eq!(
        child.inherited_for_children().direction(),
        DirectionValue::Rtl
    );
}

#[test]
fn text_indent_resolves_length_and_percentage_without_inheriting() {
    let environment = StyleEnvironment::default();
    let length = resolve_text_style(
        &declaration(
            StyleProperty::TextIndent,
            StyleValue::LengthPercentage(LengthPercentageValue::Length(LengthValue::Dimension {
                value: number(2.0),
                unit: LengthUnit::Em,
            })),
        ),
        None,
        environment,
    )
    .unwrap();
    assert_eq!(
        length.computed().text_indent(),
        ComputedTextIndent::LogicalPixels(number(28.0))
    );

    let percentage = resolve_text_style(
        &declaration(
            StyleProperty::TextIndent,
            StyleValue::LengthPercentage(LengthPercentageValue::Percentage(number(-15.0))),
        ),
        Some(length.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(
        percentage.computed().text_indent(),
        ComputedTextIndent::Percentage(number(-15.0))
    );
    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(length.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(
        child.computed().text_indent(),
        ComputedTextIndent::default()
    );

    for value in [
        LengthPercentageValue::Length(LengthValue::Dimension {
            value: number(f32::INFINITY),
            unit: LengthUnit::Em,
        }),
        LengthPercentageValue::Percentage(number(f32::NAN)),
        LengthPercentageValue::Calc(Box::new(CalcExpression::Number(number(1.0)))),
    ] {
        assert_eq!(
            resolve_text_style(
                &declaration(
                    StyleProperty::TextIndent,
                    StyleValue::LengthPercentage(value),
                ),
                None,
                environment,
            )
            .unwrap_err(),
            StyleResolutionError::InvalidPropertyValue(StyleProperty::TextIndent)
        );
    }
}

#[test]
fn wrapping_and_overflow_resolve_without_inheriting() {
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::WhiteSpace,
            StyleValue::WhiteSpace(WhiteSpaceValue::NoWrap),
        )
        .push(
            StyleProperty::WordBreak,
            StyleValue::WordBreak(WordBreakValue::BreakAll),
        )
        .push(
            StyleProperty::TextOverflow,
            StyleValue::TextOverflow(TextOverflowValue::Ellipsis),
        );
    let resolved = resolve_text_style(&specified, None, StyleEnvironment::default()).unwrap();
    assert_eq!(resolved.computed().white_space(), WhiteSpaceValue::NoWrap);
    assert_eq!(resolved.computed().word_break(), WordBreakValue::BreakAll);
    assert_eq!(
        resolved.computed().text_overflow(),
        TextOverflowValue::Ellipsis
    );

    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(resolved.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(child.computed().white_space(), WhiteSpaceValue::Normal);
    assert_eq!(child.computed().word_break(), WordBreakValue::Normal);
    assert_eq!(child.computed().text_overflow(), TextOverflowValue::Clip);
}

#[test]
fn text_shadow_resolves_inherits_clears_and_rejects_negative_blur() {
    let environment = StyleEnvironment::default();
    let shadow = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: px(1.0).into(),
            offset_y: LengthValue::Dimension {
                value: number(1.0),
                unit: LengthUnit::Em,
            }
            .into(),
            blur_radius: px(3.0).into(),
            color: ColorValue::Named("red".into()).into(),
        }),
    );
    let parent = resolve_text_style(&shadow, None, environment).unwrap();
    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    let value = inherited(&child).text_shadow().unwrap();
    assert_eq!([value.offset_x(), value.offset_y()], [1.0, 14.0]);
    assert_eq!(value.blur_radius(), 3.0);
    assert_eq!(value.color(), &ColorValue::Named("red".into()));

    let cleared = resolve_text_style(
        &declaration(
            StyleProperty::TextShadow,
            StyleValue::TextShadow(TextShadowValue::None),
        ),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert!(inherited(&cleared).text_shadow().is_none());

    let invalid = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: LengthValue::Zero.into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: px(-1.0).into(),
            color: ColorValue::Named("black".into()).into(),
        }),
    );
    assert_eq!(
        resolve_text_style(&invalid, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::TextShadow)
    );

    let invalid_offset_x = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: px(f32::NAN).into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: LengthValue::Zero.into(),
            color: ColorValue::Named("black".into()).into(),
        }),
    );
    let invalid_offset_y = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: LengthValue::Zero.into(),
            offset_y: px(f32::NAN).into(),
            blur_radius: LengthValue::Zero.into(),
            color: ColorValue::Named("black".into()).into(),
        }),
    );
    let invalid_blur = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: LengthValue::Zero.into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: px(f32::NAN).into(),
            color: ColorValue::Named("black".into()).into(),
        }),
    );
    assert_eq!(
        resolve_text_style(&invalid_offset_x, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::TextShadow)
    );
    assert_eq!(
        resolve_text_style(&invalid_offset_y, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::TextShadow)
    );
    assert_eq!(
        resolve_text_style(&invalid_blur, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::TextShadow)
    );
    let invalid_color = declaration(
        StyleProperty::TextShadow,
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: LengthValue::Zero.into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: LengthValue::Zero.into(),
            color: ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: number(f32::NAN),
            }
            .into(),
        }),
    );
    assert_eq!(
        resolve_text_style(&invalid_color, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::Color)
    );
}

#[test]
fn text_decoration_resolves_current_color_and_inherits() {
    let environment = StyleEnvironment::default();
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named("blue".into())),
        )
        .push(
            StyleProperty::TextDecoration,
            StyleValue::TextDecoration(TextDecorationValue {
                line: TextDecorationLineValue::Underline,
                style: TextDecorationStyleValue::Wavy,
                color: None,
            }),
        );
    let parent = resolve_text_style(&specified, None, environment).unwrap();
    let decoration = inherited(&parent).text_decoration();
    assert_eq!(decoration.line(), TextDecorationLineValue::Underline);
    assert_eq!(decoration.style(), TextDecorationStyleValue::Wavy);
    assert_eq!(decoration.color(), &ColorValue::Named("blue".into()));

    let child = resolve_text_style(
        &SpecifiedStyle::new(),
        Some(parent.inherited_for_children()),
        environment,
    )
    .unwrap();
    assert_eq!(inherited(&child).text_decoration(), decoration);

    let invalid_color = declaration(
        StyleProperty::TextDecoration,
        StyleValue::TextDecoration(TextDecorationValue {
            line: TextDecorationLineValue::Underline,
            style: TextDecorationStyleValue::Solid,
            color: Some(
                ColorValue::Rgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: number(f32::NAN),
                }
                .into(),
            ),
        }),
    );
    assert_eq!(
        resolve_text_style(&invalid_color, None, environment).unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::Color)
    );
}

#[test]
fn invalid_typed_values_are_rejected() {
    let cases = [
        declaration(
            StyleProperty::FontFamily,
            StyleValue::FontFamily(FontFamilyValue::Named(String::new())),
        ),
        declaration(
            StyleProperty::FontWeight,
            StyleValue::FontWeight(FontWeightValue::from_raw(0)),
        ),
        declaration(
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(length(-1.0, LengthUnit::Px)),
        ),
        declaration(
            StyleProperty::LineHeight,
            StyleValue::LineHeight(LineHeightValue::Number(number(-1.0))),
        ),
        declaration(
            StyleProperty::LineHeight,
            StyleValue::LineHeight(LineHeightValue::Number(number(f32::NAN))),
        ),
        declaration(
            StyleProperty::LineHeight,
            StyleValue::LineHeight(LineHeightValue::LengthPercentage(length(
                -1.0,
                LengthUnit::Px,
            ))),
        ),
    ];
    for style in cases {
        resolve_text_style(&style, None, StyleEnvironment::default()).unwrap_err();
    }
    assert_eq!(
        expect_length_percentage(
            StyleProperty::FontSize,
            &StyleValue::Length(LengthValue::Zero)
        )
        .unwrap_err(),
        StyleResolutionError::InvalidPropertyValue(StyleProperty::FontSize)
    );
}

#[test]
fn non_finite_lengths_and_overflow_are_rejected() {
    let environment = StyleEnvironment::default();
    assert!(
        resolve_length(
            px(f32::NAN),
            14.0,
            environment,
            StyleProperty::LetterSpacing
        )
        .is_err()
    );
    assert!(
        resolve_length(
            LengthValue::Dimension {
                value: number(f32::MAX),
                unit: LengthUnit::Vw,
            },
            14.0,
            StyleEnvironment::new(f32::MAX, 1.0, 1.0, 14.0),
            StyleProperty::LetterSpacing
        )
        .is_err()
    );
    let overflowing = LengthPercentageValue::Percentage(number(f32::MAX));
    assert!(
        resolve_length_percentage(
            &overflowing,
            f32::MAX,
            14.0,
            environment,
            StyleProperty::FontSize
        )
        .is_err()
    );

    let invalid_calc = LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
        Box::new(CalcExpression::Number(number(1.0))),
        Box::new(CalcExpression::Value(Box::new(
            LengthPercentageValue::Length(px(1.0)),
        ))),
    )));
    for (property, value) in [
        (
            StyleProperty::FontSize,
            StyleValue::LengthPercentage(invalid_calc.clone()),
        ),
        (
            StyleProperty::LineHeight,
            StyleValue::LineHeight(LineHeightValue::LengthPercentage(
                LengthPercentageValue::Percentage(number(f32::NAN)),
            )),
        ),
        (
            StyleProperty::LetterSpacing,
            StyleValue::Length(px(f32::NAN)),
        ),
    ] {
        assert!(
            resolve_text_style(
                &declaration(property, value),
                None,
                StyleEnvironment::default()
            )
            .is_err()
        );
    }
    assert_eq!(
        resolve_length_percentage(
            &invalid_calc,
            14.0,
            14.0,
            environment,
            StyleProperty::FontSize,
        )
        .unwrap_err(),
        StyleResolutionError::InvalidCalculation(StyleProperty::FontSize)
    );

    let invalid_leaf = || CalcExpression::Number(number(f32::NAN));
    let valid_leaf = || CalcExpression::Number(number(1.0));
    for expression in [
        CalcExpression::Add(Box::new(invalid_leaf()), Box::new(valid_leaf())),
        CalcExpression::Add(Box::new(valid_leaf()), Box::new(invalid_leaf())),
        CalcExpression::Sub(Box::new(invalid_leaf()), Box::new(valid_leaf())),
        CalcExpression::Sub(Box::new(valid_leaf()), Box::new(invalid_leaf())),
        CalcExpression::Mul(Box::new(invalid_leaf()), Box::new(valid_leaf())),
        CalcExpression::Mul(Box::new(valid_leaf()), Box::new(invalid_leaf())),
        CalcExpression::Div(Box::new(invalid_leaf()), Box::new(valid_leaf())),
        CalcExpression::Div(Box::new(valid_leaf()), Box::new(invalid_leaf())),
    ] {
        evaluate_calc(
            &expression,
            14.0,
            14.0,
            environment,
            StyleProperty::FontSize,
        )
        .unwrap_err();
    }
    evaluate_calc(
        &CalcExpression::Value(Box::new(LengthPercentageValue::Length(px(f32::NAN)))),
        14.0,
        14.0,
        environment,
        StyleProperty::FontSize,
    )
    .unwrap_err();
    resolve_text_style(
        &declaration(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named(String::new())),
        ),
        None,
        environment,
    )
    .unwrap_err();
}

#[test]
fn colors_are_normalized_and_validated() {
    assert_eq!(
        normalize_color(&ColorValue::Named("blue".into())).unwrap(),
        ColorValue::Named("blue".into())
    );
    assert!(normalize_color(&ColorValue::Named(String::new())).is_err());
    let rgba = ColorValue::Rgba {
        red: 1,
        green: 2,
        blue: 3,
        alpha: number(0.5),
    };
    assert_eq!(normalize_color(&rgba).unwrap(), rgba);
    for alpha in [f32::NAN, -0.1, 1.1] {
        assert!(
            normalize_color(&ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: number(alpha),
            })
            .is_err()
        );
    }
    let hsla = ColorValue::Hsla {
        hue_degrees: number(-30.0),
        saturation: number(50.0),
        lightness: number(25.0),
        alpha: number(1.0),
    };
    assert_eq!(
        normalize_color(&hsla).unwrap(),
        ColorValue::Hsla {
            hue_degrees: number(330.0),
            saturation: number(50.0),
            lightness: number(25.0),
            alpha: number(1.0),
        }
    );
    for (hue, saturation, lightness, alpha) in [
        (f32::NAN, 0.0, 0.0, 1.0),
        (0.0, f32::NAN, 0.0, 1.0),
        (0.0, -1.0, 0.0, 1.0),
        (0.0, 101.0, 0.0, 1.0),
        (0.0, 0.0, f32::NAN, 1.0),
        (0.0, 0.0, -1.0, 1.0),
        (0.0, 0.0, 101.0, 1.0),
        (0.0, 0.0, 0.0, f32::NAN),
        (0.0, 0.0, 0.0, -0.1),
        (0.0, 0.0, 0.0, 1.1),
    ] {
        assert!(
            normalize_color(&ColorValue::Hsla {
                hue_degrees: number(hue),
                saturation: number(saturation),
                lightness: number(lightness),
                alpha: number(alpha),
            })
            .is_err()
        );
    }
}

#[test]
fn inherited_change_classification_distinguishes_metrics_and_color() {
    let initial =
        resolve_text_style(&SpecifiedStyle::new(), None, StyleEnvironment::default()).unwrap();
    let initial = initial.inherited_for_children();
    let unchanged = initial.changes_from(initial);
    assert!(unchanged.is_empty());
    assert!(unchanged.properties().is_empty());
    assert!(unchanged.impacts().is_empty());

    let changed = InheritedStyle {
        custom_properties: BTreeMap::from([(
            CustomPropertyName::new("--accent").unwrap(),
            StyleValue::Color(ColorValue::Named("red".into())),
        )]),
        cursor: CursorValue::Grab,
        pointer_events: PointerEventsValue::None,
        direction: DirectionValue::Rtl,
        font_family: FontFamilyValue::Named("Inter".into()),
        font_size: number(20.0),
        font_weight: FontWeightValue::BOLD,
        font_style: FontStyleValue::Oblique,
        font_features: vec![FontFeatureValue {
            tag: crate::OpenTypeTagValue::new(*b"kern").unwrap(),
            value: 0,
        }],
        font_variations: vec![FontVariationValue {
            tag: crate::OpenTypeTagValue::new(*b"wght").unwrap(),
            value: number(650.0),
        }],
        font_optical_sizing: FontOpticalSizingValue::Auto,
        line_height: ComputedLineHeight::LogicalPixels(number(24.0)),
        letter_spacing: number(1.0),
        color: ColorValue::Named("red".into()),
        text_align: TextAlignValue::Center,
        text_decoration: ComputedTextDecoration {
            line: TextDecorationLineValue::Underline,
            style: TextDecorationStyleValue::Dashed,
            color: ColorValue::Named("green".into()),
        },
        text_shadow: Some(ComputedTextShadow {
            offset_x: number(1.0),
            offset_y: number(2.0),
            blur_radius: number(3.0),
            color: ColorValue::Named("blue".into()),
        }),
    };
    let change = changed.changes_from(initial);
    for property in [
        InheritedPropertySet::FONT_FAMILY,
        InheritedPropertySet::FONT_SIZE,
        InheritedPropertySet::FONT_WEIGHT,
        InheritedPropertySet::FONT_STYLE,
        InheritedPropertySet::FONT_FEATURE_SETTINGS,
        InheritedPropertySet::FONT_VARIATION_SETTINGS,
        InheritedPropertySet::FONT_OPTICAL_SIZING,
        InheritedPropertySet::CURSOR,
        InheritedPropertySet::POINTER_EVENTS,
        InheritedPropertySet::CUSTOM_PROPERTIES,
        InheritedPropertySet::DIRECTION,
        InheritedPropertySet::LINE_HEIGHT,
        InheritedPropertySet::LETTER_SPACING,
        InheritedPropertySet::COLOR,
        InheritedPropertySet::TEXT_ALIGN,
        InheritedPropertySet::TEXT_DECORATION,
        InheritedPropertySet::TEXT_SHADOW,
    ] {
        assert!(change.properties().contains(property));
    }
    for impact in [
        PropertyImpactSet::INTRINSIC_MEASURE,
        PropertyImpactSet::LAYOUT,
        PropertyImpactSet::PAINT,
        PropertyImpactSet::INPUT,
    ] {
        assert!(change.impacts().contains(impact));
    }
}

#[test]
fn custom_properties_inherit_and_resolve_whole_typed_values() {
    let accent = CustomPropertyName::new("--Accent").unwrap();
    let parent = resolve_style(
        &SpecifiedStyle::new().push_custom(
            accent.clone(),
            StyleValue::Color(ColorValue::Named("rebeccapurple".into())),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    let child = resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Color,
            StyleValue::Variable(CustomPropertyReference::new(accent.clone())),
        ),
        Some(parent.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        child.inherited_for_children().custom_property(&accent),
        Some(&StyleValue::Color(ColorValue::Named(
            "rebeccapurple".into()
        )))
    );
    assert_eq!(
        child.inherited_for_children().color(),
        &ColorValue::Named("rebeccapurple".into())
    );
}

#[test]
fn wrong_typed_variable_invalidates_only_the_consuming_declaration() {
    let accent = CustomPropertyName::new("--accent").unwrap();
    let parent = resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Color,
            StyleValue::Color(ColorValue::Named("blue".into())),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    let child = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(accent.clone(), StyleValue::Length(LengthValue::Zero))
            .push(
                StyleProperty::Color,
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    accent,
                    StyleValue::Color(ColorValue::Named("red".into())),
                )),
            )
            .push(
                StyleProperty::Width,
                StyleValue::Size(crate::SizeValue::LengthPercentage(
                    LengthPercentageValue::Length(LengthValue::Dimension {
                        value: StyleNumber::new(24.0),
                        unit: LengthUnit::Px,
                    }),
                )),
            ),
        Some(parent.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        child.inherited_for_children().color(),
        &ColorValue::Named("blue".into())
    );
    assert_eq!(
        child.computed().layout().size.width,
        crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(24.0, 0.0))
    );
}

#[test]
fn composite_variables_resolve_in_custom_values_and_registered_declarations() {
    let color = CustomPropertyName::new("--color").unwrap();
    let angle = CustomPropertyName::new("--angle").unwrap();
    let length = CustomPropertyName::new("--length").unwrap();
    let scale = CustomPropertyName::new("--scale").unwrap();
    let image = CustomPropertyName::new("--image").unwrap();
    let specified = SpecifiedStyle::new()
        .push_custom(
            color.clone(),
            StyleValue::Color(ColorValue::Named("red".into())),
        )
        .push_custom(angle.clone(), StyleValue::Angle(number(45.0)))
        .push_custom(length.clone(), StyleValue::Length(px(8.0)))
        .push_custom(scale.clone(), StyleValue::Number(number(2.0)))
        .push_custom(
            image.clone(),
            StyleValue::BackgroundImages(vec![crate::BackgroundImageValue::Gradient(
                crate::GradientValue::Linear {
                    angle_degrees: component_variable(&angle),
                    stops: vec![
                        crate::GradientStopValue {
                            color: component_variable(&color),
                            position: None,
                        },
                        crate::GradientStopValue {
                            color: ColorValue::Named("blue".into()).into(),
                            position: None,
                        },
                    ],
                },
            )]),
        )
        .push(
            StyleProperty::BackgroundImage,
            StyleValue::Variable(CustomPropertyReference::new(image)),
        )
        .push(
            StyleProperty::Transform,
            StyleValue::Transform(crate::TransformValue(vec![
                crate::TransformFunctionValue::Rotate(component_variable(&angle)),
                crate::TransformFunctionValue::Scale(
                    component_variable(&scale),
                    StyleNumber::new(1.0).into(),
                ),
                crate::TransformFunctionValue::TranslateZ(component_variable(&length)),
            ])),
        );

    let resolved = resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
    assert!(matches!(
        &resolved.computed().paint().background_images[0],
        crate::ComputedBackgroundImage::Gradient(crate::ComputedGradient::Linear {
            angle_degrees,
            stops,
        }) if angle_degrees.get() == 45.0
            && stops[0].color == ColorValue::Named("red".into())
    ));
    assert_eq!(resolved.computed().paint().transform.functions.len(), 3);

    let unresolved = ComponentValue::<ColorValue>::Variable(CustomPropertyReference::new(color));
    assert!(std::panic::catch_unwind(|| resolved_component(&unresolved)).is_err());
    let unresolved = ComponentValue::<LengthValue>::Variable(CustomPropertyReference::new(length));
    assert!(std::panic::catch_unwind(|| resolved_component(&unresolved)).is_err());
    let unresolved = ComponentValue::<StyleNumber>::Variable(CustomPropertyReference::new(scale));
    assert!(std::panic::catch_unwind(|| resolved_component(&unresolved)).is_err());
}

#[test]
fn missing_composite_variables_invalidate_custom_and_registered_declarations() {
    let missing = CustomPropertyName::new("--missing").unwrap();
    let nested = CustomPropertyName::new("--nested").unwrap();
    let shadow = || {
        StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x: LengthValue::Zero.into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: LengthValue::Zero.into(),
            color: component_variable(&missing),
        })
    };
    let box_shadow = || {
        StyleValue::BoxShadows(vec![crate::BoxShadowValue {
            offset_x: LengthValue::Zero.into(),
            offset_y: LengthValue::Zero.into(),
            blur_radius: LengthValue::Zero.into(),
            spread_radius: LengthValue::Zero.into(),
            color: component_variable(&missing),
            inset: false,
        }])
    };
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(nested.clone(), shadow())
            .push(StyleProperty::TextShadow, shadow())
            .push(StyleProperty::BoxShadow, box_shadow()),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    assert!(
        resolved
            .inherited_for_children()
            .custom_property(&nested)
            .is_none()
    );
    assert!(resolved.inherited_for_children().text_shadow().is_none());
    assert!(resolved.computed().paint().box_shadows.is_empty());
}

#[test]
fn direct_invalid_values_remain_resolution_errors() {
    let error = resolve_style(
        &SpecifiedStyle::new().push(StyleProperty::Color, StyleValue::Length(LengthValue::Zero)),
        None,
        StyleEnvironment::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        StyleResolutionError::InvalidPropertyValue(StyleProperty::Color)
    );
}

#[test]
fn custom_properties_support_forward_references() {
    let a = CustomPropertyName::new("--a").unwrap();
    let b = CustomPropertyName::new("--b").unwrap();
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(
                a.clone(),
                StyleValue::Variable(CustomPropertyReference::new(b.clone())),
            )
            .push_custom(
                b,
                StyleValue::Size(crate::SizeValue::LengthPercentage(
                    LengthPercentageValue::Length(LengthValue::Dimension {
                        value: number(48.0),
                        unit: LengthUnit::Px,
                    }),
                )),
            )
            .push(
                StyleProperty::Width,
                StyleValue::Variable(CustomPropertyReference::new(a)),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        resolved.computed().layout().size.width,
        crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(48.0, 0.0))
    );
}

#[test]
fn typed_calc_variable_resolution_covers_arithmetic_and_operand_types() {
    let base = CustomPropertyName::new("--base").unwrap();
    let derived = CustomPropertyName::new("--derived").unwrap();
    let arithmetic = |name: CustomPropertyName| {
        LengthPercentageValue::Calc(Box::new(CalcExpression::Div(
            Box::new(CalcExpression::Mul(
                Box::new(CalcExpression::Sub(
                    Box::new(CalcExpression::Add(
                        Box::new(CalcExpression::Variable(CustomPropertyReference::new(name))),
                        Box::new(CalcExpression::Value(Box::new(
                            LengthPercentageValue::Length(px(5.0)),
                        ))),
                    )),
                    Box::new(CalcExpression::Value(Box::new(
                        LengthPercentageValue::Length(px(1.0)),
                    ))),
                )),
                Box::new(CalcExpression::Number(number(2.0))),
            )),
            Box::new(CalcExpression::Number(number(2.0))),
        )))
    };
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(base.clone(), StyleValue::Length(px(10.0)))
            .push_custom(
                derived.clone(),
                StyleValue::LengthPercentage(arithmetic(base)),
            )
            .push(
                StyleProperty::Width,
                StyleValue::Size(crate::SizeValue::LengthPercentage(arithmetic(derived))),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().layout().size.width,
        crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(18.0, 0.0))
    );

    let operand_cases = [
        StyleValue::Length(px(3.0)),
        StyleValue::LengthPercentage(LengthPercentageValue::Length(px(3.0))),
        StyleValue::LengthPercentage(LengthPercentageValue::Calc(Box::new(
            CalcExpression::Value(Box::new(LengthPercentageValue::Length(px(3.0)))),
        ))),
    ];
    for operand in operand_cases {
        let name = CustomPropertyName::new("--operand").unwrap();
        let resolved = resolve_style(
            &SpecifiedStyle::new()
                .push_custom(name.clone(), operand)
                .push(
                    StyleProperty::Width,
                    StyleValue::Size(crate::SizeValue::LengthPercentage(
                        LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
                            Box::new(CalcExpression::Variable(CustomPropertyReference::new(name))),
                            Box::new(CalcExpression::Value(Box::new(
                                LengthPercentageValue::Length(px(1.0)),
                            ))),
                        ))),
                    )),
                ),
            None,
            StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().layout().size.width,
            crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(4.0, 0.0))
        );
    }

    let scalar = CustomPropertyName::new("--scalar").unwrap();
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(scalar.clone(), StyleValue::Number(number(3.0)))
            .push(
                StyleProperty::Width,
                StyleValue::Size(crate::SizeValue::LengthPercentage(
                    LengthPercentageValue::Calc(Box::new(CalcExpression::Mul(
                        Box::new(CalcExpression::Variable(CustomPropertyReference::new(
                            scalar,
                        ))),
                        Box::new(CalcExpression::Value(Box::new(
                            LengthPercentageValue::Length(px(2.0)),
                        ))),
                    ))),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().layout().size.width,
        crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(6.0, 0.0))
    );

    let wrong_type = CustomPropertyName::new("--wrong-type").unwrap();
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(
                wrong_type.clone(),
                StyleValue::Color(ColorValue::Named("red".into())),
            )
            .push(
                StyleProperty::Width,
                StyleValue::Size(crate::SizeValue::LengthPercentage(
                    LengthPercentageValue::Calc(Box::new(CalcExpression::Variable(
                        CustomPropertyReference::new(wrong_type),
                    ))),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.computed().layout().size.width,
        crate::ComputedSizeValue::Auto
    );
}

#[test]
fn invalid_calc_after_custom_property_substitution_drops_only_that_declaration() {
    let scalar = CustomPropertyName::new("--scalar").unwrap();
    let invalid_width = LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
        Box::new(CalcExpression::Value(Box::new(
            LengthPercentageValue::Length(px(10.0)),
        ))),
        Box::new(CalcExpression::Variable(CustomPropertyReference::new(
            scalar.clone(),
        ))),
    )));
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(scalar, StyleValue::Number(number(2.0)))
            .push(
                StyleProperty::Width,
                StyleValue::Size(crate::SizeValue::LengthPercentage(invalid_width)),
            )
            .push(
                StyleProperty::Height,
                StyleValue::Size(crate::SizeValue::LengthPercentage(
                    LengthPercentageValue::Length(px(24.0)),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        resolved.computed().layout().size.width,
        crate::ComputedSizeValue::Auto
    );
    assert_eq!(
        resolved.computed().layout().size.height,
        crate::ComputedSizeValue::Value(crate::ComputedLengthPercentage::new(24.0, 0.0))
    );
}

#[test]
fn custom_properties_materialize_inside_box_shadow_and_clip_path() {
    let length = CustomPropertyName::new("--length").unwrap();
    let color = CustomPropertyName::new("--color").unwrap();
    let clip_coordinate = || {
        LengthPercentageValue::Calc(Box::new(CalcExpression::Variable(
            CustomPropertyReference::new(length.clone()),
        )))
    };
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(length.clone(), StyleValue::Length(px(6.0)))
            .push_custom(
                color.clone(),
                StyleValue::Color(ColorValue::Named("red".into())),
            )
            .push(
                StyleProperty::BoxShadow,
                StyleValue::BoxShadows(vec![crate::BoxShadowValue {
                    offset_x: component_variable(&length),
                    offset_y: component_variable(&length),
                    blur_radius: component_variable(&length),
                    spread_radius: component_variable(&length),
                    color: component_variable(&color),
                    inset: false,
                }]),
            )
            .push(
                StyleProperty::ClipPath,
                StyleValue::ClipPath(crate::ClipPathValue::Shape {
                    reference_box: crate::ClipBoxValue::Border,
                    shape: crate::ClipShapeValue::Circle {
                        radius: clip_coordinate(),
                        center_x: clip_coordinate(),
                        center_y: clip_coordinate(),
                    },
                }),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    let shadow = &resolved.computed().paint().box_shadows[0];
    assert_eq!(shadow.offset_x.get(), 6.0);
    assert_eq!(shadow.offset_y.get(), 6.0);
    assert_eq!(shadow.blur_radius.get(), 6.0);
    assert_eq!(shadow.spread_radius.get(), 6.0);
    assert_eq!(shadow.color, ColorValue::Named("red".into()));
    assert!(matches!(
        resolved.computed().paint().clip_path.as_ref().map(|clip| &clip.shape),
        Some(crate::ComputedClipShape::Circle { radius, center })
            if *radius == crate::ComputedLengthPercentage::new(6.0, 0.0)
                && center.x == crate::ComputedLengthPercentage::new(6.0, 0.0)
                && center.y == crate::ComputedLengthPercentage::new(6.0, 0.0)
    ));
}

#[test]
fn unresolved_calc_variables_cover_numeric_and_substitution_failures() {
    let expression = CalcExpression::Variable(CustomPropertyReference::new(
        CustomPropertyName::new("--unresolved").unwrap(),
    ));
    let error = evaluate_calc(
        &expression,
        100.0,
        14.0,
        StyleEnvironment::default(),
        StyleProperty::Width,
    )
    .unwrap_err();
    assert_eq!(
        error,
        StyleResolutionError::InvalidCalculation(StyleProperty::Width)
    );

    let unresolved = || {
        CalcExpression::Variable(CustomPropertyReference::new(
            CustomPropertyName::new("--unresolved").unwrap(),
        ))
    };
    let scalar = || CalcExpression::Number(number(1.0));
    let expressions = [
        CalcExpression::Value(Box::new(LengthPercentageValue::Calc(
            Box::new(unresolved()),
        ))),
        CalcExpression::Add(Box::new(unresolved()), Box::new(scalar())),
        CalcExpression::Add(Box::new(scalar()), Box::new(unresolved())),
        CalcExpression::Sub(Box::new(unresolved()), Box::new(scalar())),
        CalcExpression::Sub(Box::new(scalar()), Box::new(unresolved())),
        CalcExpression::Mul(Box::new(unresolved()), Box::new(scalar())),
        CalcExpression::Mul(Box::new(scalar()), Box::new(unresolved())),
        CalcExpression::Div(Box::new(unresolved()), Box::new(scalar())),
        CalcExpression::Div(Box::new(scalar()), Box::new(unresolved())),
    ];
    for expression in expressions {
        assert!(resolve_calc_with(&expression, &mut |_| None).is_none());
    }
}

#[test]
fn nested_length_percentage_mapping_covers_every_common_wrapper() {
    let variable_name = CustomPropertyName::new("--length").unwrap();
    let length_percentage = || {
        LengthPercentageValue::Calc(Box::new(CalcExpression::Variable(
            CustomPropertyReference::new(variable_name.clone()),
        )))
    };
    let values = [
        (StyleValue::LengthPercentage(length_percentage()), 1),
        (
            StyleValue::Size(crate::SizeValue::LengthPercentage(length_percentage())),
            1,
        ),
        (
            StyleValue::Size(crate::SizeValue::FitContent(Some(length_percentage()))),
            1,
        ),
        (
            StyleValue::LengthPercentageAuto(crate::LengthPercentageAutoValue::LengthPercentage(
                length_percentage(),
            )),
            1,
        ),
        (
            StyleValue::FlexBasis(crate::FlexBasisValue::LengthPercentage(length_percentage())),
            1,
        ),
        (
            StyleValue::LineHeight(LineHeightValue::LengthPercentage(length_percentage())),
            1,
        ),
        (
            StyleValue::BorderRadius(crate::BorderRadiusValue {
                horizontal: length_percentage(),
                vertical: length_percentage(),
            }),
            2,
        ),
        (StyleValue::Color(ColorValue::Named("red".into())), 0),
    ];

    for (value, leaf_count) in values {
        let mut references = Vec::new();
        collect_custom_references(&value, &mut references);
        assert_eq!(references, vec![&variable_name; leaf_count]);

        let mut visited = 0;
        assert!(
            map_nested_length_percentages(&value, &mut |leaf| {
                visited += 1;
                Some(leaf.clone())
            })
            .is_some()
        );
        assert_eq!(visited, leaf_count);

        for rejected_leaf in 0..leaf_count {
            let mut visited = 0;
            assert!(
                map_nested_length_percentages(&value, &mut |leaf| {
                    let current = visited;
                    visited += 1;
                    (current != rejected_leaf).then(|| leaf.clone())
                })
                .is_none()
            );
        }
    }
}

#[test]
fn typed_calc_variables_resolve_inside_nested_paint_and_grid_values() {
    let spacing = CustomPropertyName::new("--spacing").unwrap();
    let nested = || {
        LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
            Box::new(CalcExpression::Variable(CustomPropertyReference::new(
                spacing.clone(),
            ))),
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Length(px(5.0)),
            ))),
        )))
    };
    let track = crate::GridTrackSizingValue {
        min: crate::GridMinTrackSizingValue::Fixed(nested()),
        max: crate::GridMaxTrackSizingValue::Fixed(nested()),
    };
    let specified = SpecifiedStyle::new()
        .push_custom(spacing.clone(), StyleValue::Length(px(10.0)))
        .push(
            StyleProperty::BackgroundImage,
            StyleValue::BackgroundImages(vec![crate::BackgroundImageValue::Gradient(
                crate::GradientValue::Linear {
                    angle_degrees: number(180.0).into(),
                    stops: vec![
                        crate::GradientStopValue {
                            color: ColorValue::Named("red".into()).into(),
                            position: Some(nested()),
                        },
                        crate::GradientStopValue {
                            color: ColorValue::Named("blue".into()).into(),
                            position: None,
                        },
                    ],
                },
            )]),
        )
        .push(
            StyleProperty::Transform,
            StyleValue::Transform(crate::TransformValue(vec![
                crate::TransformFunctionValue::TranslateX(nested()),
            ])),
        )
        .push(
            StyleProperty::GridAutoColumns,
            StyleValue::GridTracks(vec![track]),
        );

    let resolved = resolve_style(&specified, None, StyleEnvironment::default()).unwrap();
    let expected = crate::ComputedLengthPercentage::new(15.0, 0.0);
    assert_eq!(
        resolved.computed().paint().background_images,
        vec![crate::ComputedBackgroundImage::Gradient(
            crate::ComputedGradient::Linear {
                angle_degrees: number(180.0),
                stops: vec![
                    crate::ComputedGradientStop {
                        color: ColorValue::Named("red".into()),
                        position: Some(expected),
                    },
                    crate::ComputedGradientStop {
                        color: ColorValue::Named("blue".into()),
                        position: Some(crate::ComputedLengthPercentage::new(0.0, 1.0)),
                    },
                ],
            }
        )]
    );
    assert_eq!(
        resolved.computed().paint().transform.functions,
        vec![crate::ComputedTransformFunction::Translate {
            x: expected,
            y: crate::ComputedLengthPercentage::ZERO,
            z: number(0.0),
        }]
    );
    assert_eq!(
        resolved.computed().layout().grid_auto_columns[0],
        crate::ComputedGridTrackSizing {
            min: crate::ComputedGridMinTrackSizing::Fixed(expected),
            max: crate::ComputedGridMaxTrackSizing::Fixed(expected),
        }
    );
}

#[test]
fn cyclic_or_missing_custom_property_uses_typed_fallback() {
    let a = CustomPropertyName::new("--a").unwrap();
    let b = CustomPropertyName::new("--b").unwrap();
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(
                a.clone(),
                StyleValue::Variable(CustomPropertyReference::new(b.clone())),
            )
            .push_custom(
                b,
                StyleValue::Variable(CustomPropertyReference::new(a.clone())),
            )
            .push(
                StyleProperty::Color,
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    a,
                    StyleValue::Color(ColorValue::Named("green".into())),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        resolved.inherited_for_children().color(),
        &ColorValue::Named("green".into())
    );
    assert_eq!(
        resolved
            .inherited_for_children()
            .custom_properties()
            .count(),
        0
    );

    let self_reference = CustomPropertyName::new("--self").unwrap();
    let resolved = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(
                self_reference.clone(),
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    self_reference.clone(),
                    StyleValue::Color(ColorValue::Named("red".into())),
                )),
            )
            .push(
                StyleProperty::Color,
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    self_reference,
                    StyleValue::Color(ColorValue::Named("blue".into())),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        resolved.inherited_for_children().color(),
        &ColorValue::Named("blue".into())
    );
}

#[test]
fn custom_property_fallback_graph_covers_missing_nested_and_shared_references() {
    let missing = CustomPropertyName::new("--missing").unwrap();
    let base = CustomPropertyName::new("--base").unwrap();
    let left = CustomPropertyName::new("--left").unwrap();
    let right = CustomPropertyName::new("--right").unwrap();
    let diamond = CustomPropertyName::new("--diamond").unwrap();
    let value_fallback = CustomPropertyName::new("--value-fallback").unwrap();
    let nested_fallback = CustomPropertyName::new("--nested-fallback").unwrap();
    let purple = StyleValue::Color(ColorValue::Named("purple".into()));
    let orange = StyleValue::Color(ColorValue::Named("orange".into()));
    let variable = |name| StyleValue::Variable(CustomPropertyReference::new(name));

    let parent = resolve_style(
        &SpecifiedStyle::new()
            .push_custom(base.clone(), purple.clone())
            .push_custom(left.clone(), variable(base.clone()))
            .push_custom(right.clone(), variable(base.clone()))
            .push_custom(
                diamond.clone(),
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    left,
                    variable(right),
                )),
            )
            .push_custom(
                value_fallback.clone(),
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    missing.clone(),
                    orange.clone(),
                )),
            )
            .push_custom(
                nested_fallback.clone(),
                StyleValue::Variable(CustomPropertyReference::with_fallback(
                    missing.clone(),
                    variable(base.clone()),
                )),
            ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();

    assert_eq!(
        parent.inherited_for_children().custom_property(&diamond),
        Some(&purple)
    );
    assert_eq!(
        parent
            .inherited_for_children()
            .custom_property(&value_fallback),
        Some(&orange)
    );
    assert_eq!(
        parent
            .inherited_for_children()
            .custom_property(&nested_fallback),
        Some(&purple)
    );

    let child = resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Color,
            StyleValue::Variable(CustomPropertyReference::with_fallback(
                missing,
                variable(base),
            )),
        ),
        Some(parent.inherited_for_children()),
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        child.inherited_for_children().color(),
        &ColorValue::Named("purple".into())
    );

    let invalid_at_computed_value_time = resolve_style(
        &SpecifiedStyle::new().push(
            StyleProperty::Width,
            variable(CustomPropertyName::new("--absent").unwrap()),
        ),
        None,
        StyleEnvironment::default(),
    )
    .unwrap();
    assert_eq!(
        invalid_at_computed_value_time
            .computed()
            .layout()
            .size
            .width,
        crate::ComputedSizeValue::Auto
    );
}
