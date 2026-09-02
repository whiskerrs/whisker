use super::*;
use crate::GridTemplateRepetitionValue;

fn number(value: f32) -> StyleNumber {
    StyleNumber::new(value)
}

fn length(value: f32, unit: LengthUnit) -> LengthValue {
    LengthValue::Dimension {
        value: number(value),
        unit,
    }
}

fn px(value: f32) -> LengthPercentageValue {
    LengthPercentageValue::Length(length(value, LengthUnit::Px))
}

fn percent(value: f32) -> LengthPercentageValue {
    LengthPercentageValue::Percentage(number(value))
}

fn declaration(property: StyleProperty, value: StyleValue) -> SpecifiedStyle {
    SpecifiedStyle::new().push(property, value)
}

fn resolve(specified: &SpecifiedStyle) -> Result<ComputedLayoutStyle, StyleResolutionError> {
    resolve_layout_style(
        specified,
        20.0,
        DirectionValue::Ltr,
        StyleEnvironment::new(750.0, 400.0, 2.0, 10.0),
    )
}

#[test]
fn empty_style_uses_the_documented_layout_initials() {
    let style = resolve(&SpecifiedStyle::new()).unwrap();
    assert_eq!(style, ComputedLayoutStyle::default());
    assert_eq!(
        ComputedLengthPercentage::default(),
        ComputedLengthPercentage::ZERO
    );
    assert_eq!(style.display, DisplayValue::Flex);
    assert_eq!(style.float, FloatValue::None);
    assert_eq!(style.clear, ClearValue::None);
    assert_eq!(style.overflow, Axes::all(OverflowValue::Visible));
    assert_eq!(style.position, PositionValue::Relative);
    assert_eq!(style.direction, DirectionValue::Ltr);
    assert_eq!(style.box_sizing, BoxSizingValue::BorderBox);
    assert_eq!(style.size, Axes::all(ComputedSizeValue::Auto));
    assert_eq!(style.min_size, Axes::all(ComputedSizeValue::Auto));
    assert_eq!(style.max_size, Axes::all(ComputedSizeValue::None));
    assert_eq!(
        style.margin,
        Edges::all(ComputedLengthPercentageAuto::Value(
            ComputedLengthPercentage::ZERO
        ))
    );
    assert_eq!(style.padding, Edges::all(ComputedLengthPercentage::ZERO));
    assert_eq!(style.inset, Edges::all(ComputedLengthPercentageAuto::Auto));
    assert_eq!(style.flex_grow.get(), 0.0);
    assert_eq!(style.flex_shrink.get(), 1.0);
    assert_eq!(style.flex_basis, ComputedFlexBasis::Auto);
    assert_eq!(style.order, 0);
    assert!(style.aspect_ratio.is_none());
    assert!(style.changes_from(&style).is_empty());
}

#[test]
fn grid_declarations_resolve_to_backend_independent_values() {
    let fixed = |value: LengthPercentageValue| GridTrackSizingValue {
        min: GridMinTrackSizingValue::Fixed(value.clone()),
        max: GridMaxTrackSizingValue::Fixed(value),
    };
    let fraction = |value| GridTrackSizingValue {
        min: GridMinTrackSizingValue::Auto,
        max: GridMaxTrackSizingValue::Fraction(number(value)),
    };
    let columns = GridTemplateValue {
        components: vec![
            GridTemplateComponentValue::Track(fixed(LengthPercentageValue::Length(length(
                2.0,
                LengthUnit::Em,
            )))),
            GridTemplateComponentValue::Track(fraction(1.0)),
        ],
        line_names: vec![vec!["start".into()], Vec::new(), vec!["end".into()]],
    };
    let areas = GridTemplateAreasValue {
        areas: vec![GridTemplateAreaValue {
            name: "content".into(),
            row_start: 0,
            row_end: 1,
            column_start: 0,
            column_end: 2,
        }],
        row_count: 1,
        column_count: 2,
    };
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::GridTemplateColumns,
            StyleValue::GridTemplate(columns),
        )
        .push(
            StyleProperty::GridAutoRows,
            StyleValue::GridTracks(vec![fixed(percent(25.0))]),
        )
        .push(
            StyleProperty::GridAutoFlow,
            StyleValue::GridAutoFlow(GridAutoFlowValue::ColumnDense),
        )
        .push(
            StyleProperty::GridColumnStart,
            StyleValue::GridPlacement(GridPlacementValue::NamedLine("start".into(), 0)),
        )
        .push(
            StyleProperty::GridColumnEnd,
            StyleValue::GridPlacement(GridPlacementValue::Span(2)),
        )
        .push(
            StyleProperty::GridTemplateAreas,
            StyleValue::GridTemplateAreas(areas.clone()),
        )
        .push(
            StyleProperty::JustifyItems,
            StyleValue::AlignItems(AlignItemsValue::Center),
        )
        .push(
            StyleProperty::JustifySelf,
            StyleValue::AlignSelf(AlignSelfValue::End),
        );

    let style = resolve(&specified).unwrap();
    assert_eq!(style.grid_template_columns.components.len(), 2);
    assert_eq!(
        style.grid_template_columns.components[0],
        ComputedGridTemplateComponent::Track(ComputedGridTrackSizing::length(40.0))
    );
    assert_eq!(
        style.grid_auto_rows[0].min,
        ComputedGridMinTrackSizing::Fixed(ComputedLengthPercentage::new(0.0, 0.25))
    );
    assert_eq!(style.grid_auto_flow, GridAutoFlowValue::ColumnDense);
    assert_eq!(
        style.grid_column,
        GridPlacementLineValue {
            start: GridPlacementValue::NamedLine("start".into(), 0),
            end: GridPlacementValue::Span(2),
        }
    );
    assert_eq!(style.grid_template_areas, Some(areas));
    assert_eq!(style.justify_items, Some(AlignItemsValue::Center));
    assert_eq!(style.justify_self, Some(AlignSelfValue::End));
}

#[test]
fn invalid_grid_placements_and_overlapping_areas_are_rejected() {
    let invalid_line = SpecifiedStyle::new().push(
        StyleProperty::GridColumnStart,
        StyleValue::GridPlacement(GridPlacementValue::Line(0)),
    );
    assert_eq!(
        resolve(&invalid_line),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::GridColumnStart
        ))
    );

    let overlapping = GridTemplateAreasValue {
        areas: vec![
            GridTemplateAreaValue {
                name: "first".into(),
                row_start: 0,
                row_end: 2,
                column_start: 0,
                column_end: 2,
            },
            GridTemplateAreaValue {
                name: "second".into(),
                row_start: 1,
                row_end: 2,
                column_start: 1,
                column_end: 2,
            },
        ],
        row_count: 2,
        column_count: 2,
    };
    let invalid_areas = SpecifiedStyle::new().push(
        StyleProperty::GridTemplateAreas,
        StyleValue::GridTemplateAreas(overlapping),
    );
    assert_eq!(
        resolve(&invalid_areas),
        Err(StyleResolutionError::InvalidPropertyValue(
            StyleProperty::GridTemplateAreas
        ))
    );
}

#[test]
fn grid_helpers_and_every_track_sizing_variant_resolve() {
    assert_eq!(
        ComputedGridTrackSizing::fraction(2.0).max,
        ComputedGridMaxTrackSizing::Fraction(number(2.0))
    );
    assert_eq!(
        ComputedGridTrackSizing::auto(),
        ComputedGridTrackSizing {
            min: ComputedGridMinTrackSizing::Auto,
            max: ComputedGridMaxTrackSizing::Auto,
        }
    );
    assert_eq!(
        ComputedGridTemplate::tracks([ComputedGridTrackSizing::length(10.0)])
            .components
            .len(),
        1
    );
    assert_eq!(
        GridPlacementLineValue::lines(1, 3),
        GridPlacementLineValue {
            start: GridPlacementValue::Line(1),
            end: GridPlacementValue::Line(3),
        }
    );

    let fixed = |value: LengthPercentageValue| GridTrackSizingValue {
        min: GridMinTrackSizingValue::Fixed(value.clone()),
        max: GridMaxTrackSizingValue::Fixed(value),
    };
    let variants = vec![
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::MinContent,
            max: GridMaxTrackSizingValue::MinContent,
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::MaxContent,
            max: GridMaxTrackSizingValue::MaxContent,
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::Auto,
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Fixed(px(10.0)),
            max: GridMaxTrackSizingValue::FitContent(percent(50.0)),
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::Fraction(number(2.0)),
        },
    ];
    let repeated = GridTemplateValue {
        components: vec![GridTemplateComponentValue::Repeat(
            GridTemplateRepetitionValue {
                count: GridRepetitionCountValue::AutoFit,
                tracks: vec![fixed(px(20.0))],
                line_names: vec![vec!["repeat-start".into()], vec!["repeat-end".into()]],
            },
        )],
        line_names: vec![vec!["outer-start".into()], vec!["outer-end".into()]],
    };
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::GridTemplateRows,
            StyleValue::GridTemplate(repeated),
        )
        .push(
            StyleProperty::GridAutoColumns,
            StyleValue::GridTracks(variants),
        )
        .push(
            StyleProperty::GridRowStart,
            StyleValue::GridPlacement(GridPlacementValue::NamedLine("outer-start".into(), 0)),
        )
        .push(
            StyleProperty::GridRowEnd,
            StyleValue::GridPlacement(GridPlacementValue::NamedSpan("outer-end".into(), 0)),
        );
    let style = resolve(&specified).unwrap();
    assert!(matches!(
        style.grid_template_rows.components.as_slice(),
        [ComputedGridTemplateComponent::Repeat(_)]
    ));
    assert_eq!(style.grid_auto_columns.len(), 5);
    assert_eq!(
        style.grid_row,
        GridPlacementLineValue {
            start: GridPlacementValue::NamedLine("outer-start".into(), 0),
            end: GridPlacementValue::NamedSpan("outer-end".into(), 0),
        }
    );
}

#[test]
fn every_invalid_grid_shape_is_rejected() {
    let fixed = || GridTrackSizingValue {
        min: GridMinTrackSizingValue::Fixed(px(10.0)),
        max: GridMaxTrackSizingValue::Fixed(px(10.0)),
    };
    for template in [
        GridTemplateValue {
            components: vec![GridTemplateComponentValue::Repeat(
                GridTemplateRepetitionValue {
                    count: GridRepetitionCountValue::Count(0),
                    tracks: vec![fixed()],
                    line_names: vec![Vec::new(), Vec::new()],
                },
            )],
            line_names: vec![Vec::new(), Vec::new()],
        },
        GridTemplateValue {
            components: vec![GridTemplateComponentValue::Repeat(
                GridTemplateRepetitionValue {
                    count: GridRepetitionCountValue::AutoFill,
                    tracks: Vec::new(),
                    line_names: vec![Vec::new()],
                },
            )],
            line_names: vec![Vec::new(), Vec::new()],
        },
        GridTemplateValue {
            components: vec![GridTemplateComponentValue::Repeat(
                GridTemplateRepetitionValue {
                    count: GridRepetitionCountValue::Count(2),
                    tracks: vec![fixed()],
                    line_names: vec![Vec::new()],
                },
            )],
            line_names: vec![Vec::new(), Vec::new()],
        },
        GridTemplateValue {
            components: vec![GridTemplateComponentValue::Track(fixed())],
            line_names: vec![Vec::new()],
        },
        GridTemplateValue {
            components: vec![GridTemplateComponentValue::Repeat(
                GridTemplateRepetitionValue {
                    count: GridRepetitionCountValue::Count(1),
                    tracks: vec![GridTrackSizingValue {
                        min: GridMinTrackSizingValue::Fixed(px(f32::NAN)),
                        max: GridMaxTrackSizingValue::Auto,
                    }],
                    line_names: vec![Vec::new(), Vec::new()],
                },
            )],
            line_names: vec![Vec::new(), Vec::new()],
        },
    ] {
        assert!(
            resolve(&declaration(
                StyleProperty::GridTemplateColumns,
                StyleValue::GridTemplate(template)
            ))
            .is_err()
        );
    }

    for track in [
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Fixed(px(f32::NAN)),
            max: GridMaxTrackSizingValue::Auto,
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::FitContent(px(f32::NAN)),
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::Fixed(px(f32::NAN)),
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::Fraction(number(-1.0)),
        },
        GridTrackSizingValue {
            min: GridMinTrackSizingValue::Auto,
            max: GridMaxTrackSizingValue::Fraction(number(f32::NAN)),
        },
    ] {
        assert!(
            resolve(&declaration(
                StyleProperty::GridAutoRows,
                StyleValue::GridTracks(vec![track])
            ))
            .is_err()
        );
    }

    for placement in [
        GridPlacementValue::Span(0),
        GridPlacementValue::NamedLine(String::new(), 0),
        GridPlacementValue::NamedLine("two words".into(), 1),
        GridPlacementValue::NamedSpan(String::new(), 0),
        GridPlacementValue::NamedSpan("two words".into(), 1),
    ] {
        assert!(
            resolve(&declaration(
                StyleProperty::GridRowStart,
                StyleValue::GridPlacement(placement)
            ))
            .is_err()
        );
    }
}

#[test]
fn every_invalid_named_area_shape_is_rejected() {
    let area = |name: &str, row_start, row_end, column_start, column_end| GridTemplateAreaValue {
        name: name.into(),
        row_start,
        row_end,
        column_start,
        column_end,
    };
    let cases = [
        GridTemplateAreasValue {
            areas: vec![],
            row_count: 0,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![],
            row_count: 1,
            column_count: 0,
        },
        GridTemplateAreasValue {
            areas: vec![area("", 0, 1, 0, 1)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("two words", 0, 1, 0, 1)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("bad-row", 1, 1, 0, 1)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("row-bounds", 0, 2, 0, 1)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("bad-column", 0, 1, 1, 1)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("column-bounds", 0, 1, 0, 2)],
            row_count: 1,
            column_count: 1,
        },
        GridTemplateAreasValue {
            areas: vec![area("same", 0, 1, 0, 1), area("same", 1, 2, 0, 1)],
            row_count: 2,
            column_count: 1,
        },
    ];
    for areas in cases {
        assert!(
            resolve(&declaration(
                StyleProperty::GridTemplateAreas,
                StyleValue::GridTemplateAreas(areas)
            ))
            .is_err()
        );
    }
}

#[test]
fn block_float_declarations_resolve_without_backend_types() {
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::Display,
            StyleValue::Display(DisplayValue::FlowRoot),
        )
        .push(StyleProperty::Float, StyleValue::Float(FloatValue::Right))
        .push(StyleProperty::Clear, StyleValue::Clear(ClearValue::Both))
        .push(
            StyleProperty::OverflowX,
            StyleValue::Overflow(OverflowValue::Hidden),
        )
        .push(
            StyleProperty::OverflowY,
            StyleValue::Overflow(OverflowValue::Hidden),
        );
    let style = resolve(&specified).unwrap();
    assert_eq!(style.display, DisplayValue::FlowRoot);
    assert_eq!(style.float, FloatValue::Right);
    assert_eq!(style.clear, ClearValue::Both);
    assert_eq!(style.overflow, Axes::all(OverflowValue::Hidden));
}

#[test]
fn complete_box_and_flex_style_resolves_without_backend_types() {
    let specified = SpecifiedStyle::new()
        .push(
            StyleProperty::Display,
            StyleValue::Display(DisplayValue::Flex),
        )
        .push(
            StyleProperty::Position,
            StyleValue::Position(PositionValue::Absolute),
        )
        .push(
            StyleProperty::Direction,
            StyleValue::Direction(DirectionValue::Rtl),
        )
        .push(
            StyleProperty::BoxSizing,
            StyleValue::BoxSizing(BoxSizingValue::ContentBox),
        )
        .push(
            StyleProperty::Width,
            StyleValue::Size(SizeValue::LengthPercentage(px(100.0))),
        )
        .push(StyleProperty::Height, StyleValue::Size(SizeValue::Auto))
        .push(
            StyleProperty::MinWidth,
            StyleValue::Size(SizeValue::MaxContent),
        )
        .push(
            StyleProperty::MinHeight,
            StyleValue::Size(SizeValue::MinContent),
        )
        .push(
            StyleProperty::MaxWidth,
            StyleValue::Size(SizeValue::FitContent(Some(percent(50.0)))),
        )
        .push(StyleProperty::MaxHeight, StyleValue::Size(SizeValue::None))
        .push(
            StyleProperty::MarginTop,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::Auto),
        )
        .push(
            StyleProperty::MarginRight,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(2.0))),
        )
        .push(
            StyleProperty::MarginBottom,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(percent(
                3.0,
            ))),
        )
        .push(
            StyleProperty::MarginLeft,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(-4.0))),
        )
        .push(
            StyleProperty::PaddingTop,
            StyleValue::LengthPercentage(px(5.0)),
        )
        .push(
            StyleProperty::PaddingRight,
            StyleValue::Length(length(6.0, LengthUnit::Px)),
        )
        .push(
            StyleProperty::PaddingBottom,
            StyleValue::LengthPercentage(percent(7.0)),
        )
        .push(
            StyleProperty::PaddingLeft,
            StyleValue::LengthPercentage(px(8.0)),
        )
        .push(
            StyleProperty::BorderTopWidth,
            StyleValue::LengthPercentage(px(1.0)),
        )
        .push(
            StyleProperty::BorderRightWidth,
            StyleValue::Length(length(2.0, LengthUnit::Px)),
        )
        .push(
            StyleProperty::BorderBottomWidth,
            StyleValue::LengthPercentage(px(3.0)),
        )
        .push(
            StyleProperty::BorderLeftWidth,
            StyleValue::LengthPercentage(px(4.0)),
        )
        .push(
            StyleProperty::FlexDirection,
            StyleValue::FlexDirection(FlexDirectionValue::Column),
        )
        .push(
            StyleProperty::FlexWrap,
            StyleValue::FlexWrap(FlexWrapValue::Wrap),
        )
        .push(StyleProperty::FlexGrow, StyleValue::Number(number(2.0)))
        .push(StyleProperty::FlexShrink, StyleValue::Number(number(0.5)))
        .push(
            StyleProperty::FlexBasis,
            StyleValue::FlexBasis(FlexBasisValue::LengthPercentage(percent(25.0))),
        )
        .push(
            StyleProperty::JustifyContent,
            StyleValue::JustifyContent(JustifyContentValue::SpaceBetween),
        )
        .push(
            StyleProperty::AlignItems,
            StyleValue::AlignItems(AlignItemsValue::Center),
        )
        .push(
            StyleProperty::AlignSelf,
            StyleValue::AlignSelf(AlignSelfValue::Baseline),
        )
        .push(
            StyleProperty::AlignContent,
            StyleValue::AlignContent(AlignContentValue::SpaceAround),
        )
        .push(StyleProperty::RowGap, StyleValue::LengthPercentage(px(9.0)))
        .push(
            StyleProperty::ColumnGap,
            StyleValue::LengthPercentage(percent(10.0)),
        )
        .push(
            StyleProperty::AspectRatio,
            StyleValue::AspectRatio(AspectRatioValue::new(16.0, 9.0)),
        )
        .push(StyleProperty::Order, StyleValue::Integer(-3));
    let style = resolve(&specified).unwrap();
    assert_eq!(style.display, DisplayValue::Flex);
    assert_eq!(style.position, PositionValue::Absolute);
    assert_eq!(style.direction, DirectionValue::Rtl);
    assert_eq!(style.box_sizing, BoxSizingValue::ContentBox);
    assert_eq!(
        style.size.width,
        ComputedSizeValue::Value(ComputedLengthPercentage::new(100.0, 0.0))
    );
    assert_eq!(style.min_size.width, ComputedSizeValue::MaxContent);
    assert_eq!(style.min_size.height, ComputedSizeValue::MinContent);
    assert_eq!(
        style.max_size.width,
        ComputedSizeValue::FitContent(Some(ComputedLengthPercentage::new(0.0, 0.5)))
    );
    assert_eq!(style.margin.top, ComputedLengthPercentageAuto::Auto);
    assert_eq!(
        style.margin.left,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(-4.0, 0.0))
    );
    assert_eq!(style.padding.right.length(), 6.0);
    assert_eq!(style.padding.bottom.fraction(), 0.07);
    assert_eq!(style.border.top.length(), 1.0);
    assert_eq!(style.border.right.length(), 2.0);
    assert_eq!(style.border.bottom.length(), 3.0);
    assert_eq!(style.border.left.length(), 4.0);
    assert_eq!(style.flex_direction, FlexDirectionValue::Column);
    assert_eq!(style.flex_wrap, FlexWrapValue::Wrap);
    assert_eq!(style.flex_grow.get(), 2.0);
    assert_eq!(style.flex_shrink.get(), 0.5);
    assert_eq!(
        style.flex_basis,
        ComputedFlexBasis::Value(ComputedLengthPercentage::new(0.0, 0.25))
    );
    assert_eq!(style.justify_content, JustifyContentValue::SpaceBetween);
    assert_eq!(style.align_items, AlignItemsValue::Center);
    assert_eq!(style.align_self, AlignSelfValue::Baseline);
    assert_eq!(style.align_content, AlignContentValue::SpaceAround);
    assert_eq!(style.gap.height.length(), 9.0);
    assert_eq!(style.gap.width.fraction(), 0.1);
    assert_eq!(style.aspect_ratio.unwrap().get(), 16.0 / 9.0);
    assert_eq!(style.order, -3);
    assert_eq!(
        style.changes_from(&ComputedLayoutStyle::default()),
        PropertyImpactSet::LAYOUT
    );
}

#[test]
fn size_and_flex_basis_keywords_remain_distinct() {
    assert_eq!(
        resolve_size(
            &SizeValue::FitContent(None),
            20.0,
            StyleEnvironment::default(),
            StyleProperty::Width
        )
        .unwrap(),
        ComputedSizeValue::FitContent(None)
    );
    for (specified, computed) in [
        (FlexBasisValue::Auto, ComputedFlexBasis::Auto),
        (FlexBasisValue::Content, ComputedFlexBasis::Content),
    ] {
        let style = resolve(&declaration(
            StyleProperty::FlexBasis,
            StyleValue::FlexBasis(specified),
        ))
        .unwrap();
        assert_eq!(style.flex_basis, computed);
    }
}

#[test]
fn logical_insets_resolve_in_final_write_order_for_both_directions() {
    let ltr = SpecifiedStyle::new()
        .push(StyleProperty::Left, StyleValue::LengthPercentage(px(1.0)))
        .push(
            StyleProperty::InsetInlineStart,
            StyleValue::LengthPercentage(px(2.0)),
        )
        .push(StyleProperty::Right, StyleValue::LengthPercentage(px(3.0)))
        .push(
            StyleProperty::InsetInlineEnd,
            StyleValue::LengthPercentage(px(4.0)),
        )
        .push(
            StyleProperty::Top,
            StyleValue::Length(length(5.0, LengthUnit::Px)),
        )
        .push(StyleProperty::Bottom, StyleValue::LengthPercentage(px(6.0)));
    let style = resolve(&ltr).unwrap();
    assert_eq!(
        style.inset.left,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(2.0, 0.0))
    );
    assert_eq!(
        style.inset.right,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(4.0, 0.0))
    );
    assert_eq!(
        style.inset.top,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(5.0, 0.0))
    );
    assert_eq!(
        style.inset.bottom,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(6.0, 0.0))
    );

    let rtl = SpecifiedStyle::new()
        .push(
            StyleProperty::Direction,
            StyleValue::Direction(DirectionValue::Rtl),
        )
        .push(
            StyleProperty::InsetInlineStart,
            StyleValue::LengthPercentage(px(7.0)),
        )
        .push(
            StyleProperty::InsetInlineEnd,
            StyleValue::LengthPercentage(px(8.0)),
        );
    let style = resolve(&rtl).unwrap();
    assert_eq!(
        style.inset.right,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(7.0, 0.0))
    );
    assert_eq!(
        style.inset.left,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(8.0, 0.0))
    );
}

#[test]
fn logical_margin_and_padding_resolve_in_final_write_order_for_both_directions() {
    let ltr = SpecifiedStyle::new()
        .push(
            StyleProperty::MarginLeft,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(1.0))),
        )
        .push(
            StyleProperty::MarginInlineStart,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(2.0))),
        )
        .push(
            StyleProperty::MarginRight,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(3.0))),
        )
        .push(
            StyleProperty::MarginInlineEnd,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::Auto),
        )
        .push(
            StyleProperty::PaddingLeft,
            StyleValue::LengthPercentage(px(5.0)),
        )
        .push(
            StyleProperty::PaddingInlineStart,
            StyleValue::LengthPercentage(px(6.0)),
        )
        .push(
            StyleProperty::PaddingRight,
            StyleValue::LengthPercentage(px(7.0)),
        )
        .push(
            StyleProperty::PaddingInlineEnd,
            StyleValue::LengthPercentage(px(8.0)),
        );
    let style = resolve(&ltr).unwrap();
    assert_eq!(
        style.margin.left,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(2.0, 0.0))
    );
    assert_eq!(style.margin.right, ComputedLengthPercentageAuto::Auto);
    assert_eq!(style.padding.left, ComputedLengthPercentage::new(6.0, 0.0));
    assert_eq!(style.padding.right, ComputedLengthPercentage::new(8.0, 0.0));

    let rtl = SpecifiedStyle::new()
        .push(
            StyleProperty::Direction,
            StyleValue::Direction(DirectionValue::Rtl),
        )
        .push(
            StyleProperty::MarginInlineStart,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(9.0))),
        )
        .push(
            StyleProperty::MarginRight,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(10.0))),
        )
        .push(
            StyleProperty::MarginInlineEnd,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(11.0))),
        )
        .push(
            StyleProperty::PaddingInlineStart,
            StyleValue::LengthPercentage(px(12.0)),
        )
        .push(
            StyleProperty::PaddingInlineEnd,
            StyleValue::LengthPercentage(px(13.0)),
        );
    let style = resolve(&rtl).unwrap();
    assert_eq!(
        style.margin.right,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(10.0, 0.0))
    );
    assert_eq!(
        style.margin.left,
        ComputedLengthPercentageAuto::Value(ComputedLengthPercentage::new(11.0, 0.0))
    );
    assert_eq!(
        style.padding.right,
        ComputedLengthPercentage::new(12.0, 0.0)
    );
    assert_eq!(style.padding.left, ComputedLengthPercentage::new(13.0, 0.0));

    for property in [
        StyleProperty::MarginInlineStart,
        StyleProperty::MarginInlineEnd,
        StyleProperty::PaddingInlineStart,
        StyleProperty::PaddingInlineEnd,
    ] {
        assert_eq!(
            resolve(&declaration(
                property,
                StyleValue::Color(crate::ColorValue::Named("red".into())),
            )),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
}

#[test]
fn relative_units_become_logical_pixel_components() {
    let environment = StyleEnvironment::new(750.0, 400.0, 2.0, 10.0);
    let cases = [
        (LengthValue::Zero, 0.0),
        (length(2.0, LengthUnit::Px), 2.0),
        (length(2.0, LengthUnit::Em), 40.0),
        (length(2.0, LengthUnit::Rem), 20.0),
        (length(2.0, LengthUnit::Vh), 8.0),
        (length(2.0, LengthUnit::Vw), 15.0),
    ];
    for (value, expected) in cases {
        assert_eq!(
            resolve_absolute(value, 20.0, environment, StyleProperty::Width).unwrap(),
            expected
        );
    }
}

#[test]
fn affine_calc_preserves_mixed_length_and_percentage() {
    let value = LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
        Box::new(CalcExpression::Value(Box::new(px(10.0)))),
        Box::new(CalcExpression::Value(Box::new(percent(25.0)))),
    )));
    let computed = resolve_affine(
        &value,
        20.0,
        StyleEnvironment::default(),
        StyleProperty::Width,
    )
    .unwrap();
    assert_eq!(computed.length(), 10.0);
    assert_eq!(computed.fraction(), 0.25);

    let leaf = || CalcExpression::Value(Box::new(px(4.0)));
    let scalar = |value| CalcExpression::Number(number(value));
    for (expression, expected) in [
        (CalcExpression::Sub(Box::new(leaf()), Box::new(leaf())), 0.0),
        (
            CalcExpression::Mul(Box::new(scalar(2.0)), Box::new(leaf())),
            8.0,
        ),
        (
            CalcExpression::Mul(Box::new(leaf()), Box::new(scalar(3.0))),
            12.0,
        ),
        (
            CalcExpression::Div(Box::new(leaf()), Box::new(scalar(2.0))),
            2.0,
        ),
    ] {
        let computed = resolve_affine(
            &LengthPercentageValue::Calc(Box::new(expression)),
            20.0,
            StyleEnvironment::default(),
            StyleProperty::Width,
        )
        .unwrap();
        assert_eq!(computed.length(), expected);
    }
}

#[test]
fn scalar_calc_branches_and_invalid_dimensions_are_diagnostic() {
    let environment = StyleEnvironment::default();
    let scalar = |value| CalcExpression::Number(number(value));
    let affine = || CalcExpression::Value(Box::new(px(2.0)));
    evaluate_affine_calc(
        &CalcExpression::Variable(crate::CustomPropertyReference::new(
            crate::CustomPropertyName::new("--unresolved").unwrap(),
        )),
        20.0,
        environment,
        StyleProperty::Width,
    )
    .unwrap_err();
    for expression in [
        CalcExpression::Add(Box::new(scalar(1.0)), Box::new(scalar(2.0))),
        CalcExpression::Sub(Box::new(scalar(3.0)), Box::new(scalar(1.0))),
        CalcExpression::Mul(Box::new(scalar(2.0)), Box::new(scalar(3.0))),
        CalcExpression::Div(Box::new(scalar(6.0)), Box::new(scalar(2.0))),
    ] {
        evaluate_affine_calc(&expression, 20.0, environment, StyleProperty::Width).unwrap();
    }
    for expression in [
        CalcExpression::Add(Box::new(scalar(1.0)), Box::new(affine())),
        CalcExpression::Sub(Box::new(affine()), Box::new(scalar(1.0))),
        CalcExpression::Mul(Box::new(affine()), Box::new(affine())),
        CalcExpression::Div(Box::new(affine()), Box::new(scalar(0.0))),
        CalcExpression::Div(Box::new(scalar(1.0)), Box::new(affine())),
    ] {
        evaluate_affine_calc(&expression, 20.0, environment, StyleProperty::Width).unwrap_err();
    }
    resolve_affine(
        &LengthPercentageValue::Calc(Box::new(scalar(1.0))),
        20.0,
        environment,
        StyleProperty::Width,
    )
    .unwrap_err();
}

#[test]
fn invalid_types_are_reported_for_each_layout_property_family() {
    let properties = [
        StyleProperty::Display,
        StyleProperty::Float,
        StyleProperty::Clear,
        StyleProperty::OverflowX,
        StyleProperty::OverflowY,
        StyleProperty::Position,
        StyleProperty::Direction,
        StyleProperty::BoxSizing,
        StyleProperty::Width,
        StyleProperty::Height,
        StyleProperty::MinWidth,
        StyleProperty::MinHeight,
        StyleProperty::MaxWidth,
        StyleProperty::MaxHeight,
        StyleProperty::MarginTop,
        StyleProperty::MarginRight,
        StyleProperty::MarginBottom,
        StyleProperty::MarginLeft,
        StyleProperty::PaddingTop,
        StyleProperty::PaddingRight,
        StyleProperty::PaddingBottom,
        StyleProperty::PaddingLeft,
        StyleProperty::BorderTopWidth,
        StyleProperty::BorderRightWidth,
        StyleProperty::BorderBottomWidth,
        StyleProperty::BorderLeftWidth,
        StyleProperty::BorderInlineStartWidth,
        StyleProperty::BorderInlineEndWidth,
        StyleProperty::Top,
        StyleProperty::Right,
        StyleProperty::Bottom,
        StyleProperty::Left,
        StyleProperty::InsetInlineStart,
        StyleProperty::InsetInlineEnd,
        StyleProperty::FlexDirection,
        StyleProperty::FlexWrap,
        StyleProperty::FlexGrow,
        StyleProperty::FlexShrink,
        StyleProperty::FlexBasis,
        StyleProperty::JustifyContent,
        StyleProperty::AlignItems,
        StyleProperty::AlignSelf,
        StyleProperty::JustifyItems,
        StyleProperty::JustifySelf,
        StyleProperty::AlignContent,
        StyleProperty::RowGap,
        StyleProperty::ColumnGap,
        StyleProperty::AspectRatio,
        StyleProperty::Order,
        StyleProperty::GridTemplateColumns,
        StyleProperty::GridTemplateRows,
        StyleProperty::GridAutoColumns,
        StyleProperty::GridAutoRows,
        StyleProperty::GridAutoFlow,
        StyleProperty::GridTemplateAreas,
        StyleProperty::GridColumnStart,
        StyleProperty::GridColumnEnd,
        StyleProperty::GridRowStart,
        StyleProperty::GridRowEnd,
    ];
    for property in properties {
        assert_eq!(
            resolve(&declaration(property, StyleValue::Bool(true))).unwrap_err(),
            StyleResolutionError::InvalidPropertyValue(property)
        );
    }
}

#[test]
fn invalid_numbers_are_rejected_or_clamped_by_property_semantics() {
    let negative = resolve(&declaration(
        StyleProperty::FlexGrow,
        StyleValue::Number(number(-2.0)),
    ))
    .unwrap();
    assert_eq!(negative.flex_grow.get(), 0.0);
    for (property, value) in [
        (
            StyleProperty::FlexShrink,
            StyleValue::Number(number(f32::NAN)),
        ),
        (
            StyleProperty::Width,
            StyleValue::Size(SizeValue::LengthPercentage(percent(f32::NAN))),
        ),
        (
            StyleProperty::PaddingTop,
            StyleValue::LengthPercentage(px(f32::INFINITY)),
        ),
        (
            StyleProperty::BorderTopWidth,
            StyleValue::LengthPercentage(px(-1.0)),
        ),
        (StyleProperty::Order, StyleValue::Integer(i64::MAX)),
    ] {
        resolve(&declaration(property, value)).unwrap_err();
    }
    for ratio in [
        AspectRatioValue::new(f32::NAN, 1.0),
        AspectRatioValue::new(1.0, f32::NAN),
        AspectRatioValue::new(0.0, 1.0),
        AspectRatioValue::new(1.0, 0.0),
        AspectRatioValue::new(f32::MAX, f32::MIN_POSITIVE),
    ] {
        resolve_aspect_ratio(ratio).unwrap_err();
    }
    assert_eq!(AspectRatioValue::new(4.0, 3.0).width(), 4.0);
    assert_eq!(AspectRatioValue::new(4.0, 3.0).height(), 3.0);
}

#[test]
fn border_width_accepts_lengths_and_rejects_percentage_components() {
    let property = StyleProperty::BorderTopWidth;
    let pure_length = LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
        Box::new(CalcExpression::Value(Box::new(px(2.0)))),
        Box::new(CalcExpression::Value(Box::new(px(3.0)))),
    )));
    assert_eq!(
        resolve(&declaration(
            property,
            StyleValue::LengthPercentage(pure_length),
        ))
        .unwrap()
        .border
        .top
        .length(),
        5.0
    );

    for value in [
        percent(10.0),
        LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
            Box::new(CalcExpression::Value(Box::new(px(2.0)))),
            Box::new(CalcExpression::Value(Box::new(percent(10.0)))),
        ))),
        LengthPercentageValue::Calc(Box::new(CalcExpression::Sub(
            Box::new(CalcExpression::Value(Box::new(percent(10.0)))),
            Box::new(CalcExpression::Value(Box::new(percent(10.0)))),
        ))),
    ] {
        assert_eq!(
            resolve(&declaration(property, StyleValue::LengthPercentage(value),)),
            Err(StyleResolutionError::InvalidPropertyValue(property))
        );
    }
}

#[test]
fn nested_calc_errors_propagate_from_both_operands() {
    let bad = || CalcExpression::Number(number(f32::NAN));
    let good = || CalcExpression::Number(number(1.0));
    for expression in [
        CalcExpression::Add(Box::new(bad()), Box::new(good())),
        CalcExpression::Add(Box::new(good()), Box::new(bad())),
        CalcExpression::Sub(Box::new(bad()), Box::new(good())),
        CalcExpression::Sub(Box::new(good()), Box::new(bad())),
        CalcExpression::Mul(Box::new(bad()), Box::new(good())),
        CalcExpression::Mul(Box::new(good()), Box::new(bad())),
        CalcExpression::Div(Box::new(bad()), Box::new(good())),
        CalcExpression::Div(Box::new(good()), Box::new(bad())),
    ] {
        evaluate_affine_calc(
            &expression,
            20.0,
            StyleEnvironment::default(),
            StyleProperty::Width,
        )
        .unwrap_err();
    }
    evaluate_affine_calc(
        &CalcExpression::Value(Box::new(px(f32::NAN))),
        20.0,
        StyleEnvironment::default(),
        StyleProperty::Width,
    )
    .unwrap_err();
}

#[test]
fn nested_layout_value_failures_propagate_through_public_resolution() {
    for specified in [
        declaration(
            StyleProperty::FlexBasis,
            StyleValue::FlexBasis(FlexBasisValue::LengthPercentage(px(f32::NAN))),
        ),
        declaration(
            StyleProperty::AspectRatio,
            StyleValue::AspectRatio(AspectRatioValue::new(0.0, 1.0)),
        ),
        declaration(
            StyleProperty::Width,
            StyleValue::Size(SizeValue::FitContent(Some(px(f32::NAN)))),
        ),
        declaration(
            StyleProperty::Width,
            StyleValue::Size(SizeValue::LengthPercentage(LengthPercentageValue::Calc(
                Box::new(CalcExpression::Add(
                    Box::new(CalcExpression::Number(number(1.0))),
                    Box::new(CalcExpression::Value(Box::new(px(1.0)))),
                )),
            ))),
        ),
        declaration(
            StyleProperty::MarginTop,
            StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(
                f32::NAN,
            ))),
        ),
        declaration(
            StyleProperty::InsetInlineStart,
            StyleValue::LengthPercentage(px(f32::NAN)),
        ),
        declaration(
            StyleProperty::Left,
            StyleValue::Length(length(f32::NAN, LengthUnit::Px)),
        ),
    ] {
        resolve(&specified).unwrap_err();
    }
}

#[test]
fn arithmetic_overflow_is_rejected_after_dimension_resolution() {
    let overflowing_calc = LengthPercentageValue::Calc(Box::new(CalcExpression::Add(
        Box::new(CalcExpression::Value(Box::new(px(f32::MAX)))),
        Box::new(CalcExpression::Value(Box::new(px(f32::MAX)))),
    )));
    resolve(&declaration(
        StyleProperty::Width,
        StyleValue::Size(SizeValue::LengthPercentage(overflowing_calc)),
    ))
    .unwrap_err();

    resolve_layout_style(
        &declaration(
            StyleProperty::Width,
            StyleValue::Size(SizeValue::LengthPercentage(LengthPercentageValue::Length(
                length(f32::MAX, LengthUnit::Em),
            ))),
        ),
        f32::MAX,
        DirectionValue::Ltr,
        StyleEnvironment::default(),
    )
    .unwrap_err();
}
