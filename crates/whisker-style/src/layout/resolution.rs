use super::*;

pub(super) fn resolve_optional_grid_template(
    value: Option<&StyleValue>,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedGridTemplate, StyleResolutionError> {
    match value {
        Some(StyleValue::GridTemplate(value)) => {
            resolve_grid_template(value, font_size, environment, property)
        }
        Some(_) => Err(invalid(property)),
        None => Ok(ComputedGridTemplate::default()),
    }
}

pub(super) fn resolve_grid_template(
    value: &GridTemplateValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedGridTemplate, StyleResolutionError> {
    let components = value
        .components
        .iter()
        .map(|component| match component {
            GridTemplateComponentValue::Track(track) => {
                resolve_grid_track(track, font_size, environment, property)
                    .map(ComputedGridTemplateComponent::Track)
            }
            GridTemplateComponentValue::Repeat(repetition) => {
                if matches!(repetition.count, GridRepetitionCountValue::Count(0)) {
                    return Err(invalid(property));
                }
                let tracks = repetition
                    .tracks
                    .iter()
                    .map(|track| resolve_grid_track(track, font_size, environment, property))
                    .collect::<Result<Vec<_>, _>>()?;
                if tracks.is_empty() || repetition.line_names.len() != tracks.len() + 1 {
                    return Err(invalid(property));
                }
                Ok(ComputedGridTemplateComponent::Repeat(
                    ComputedGridTemplateRepetition {
                        count: repetition.count,
                        tracks,
                        line_names: repetition.line_names.clone(),
                    },
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if value.line_names.len() != components.len() + 1 {
        return Err(invalid(property));
    }
    Ok(ComputedGridTemplate {
        components,
        line_names: value.line_names.clone(),
    })
}

pub(super) fn resolve_optional_grid_tracks(
    value: Option<&StyleValue>,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<Vec<ComputedGridTrackSizing>, StyleResolutionError> {
    match value {
        Some(StyleValue::GridTracks(value)) => value
            .iter()
            .map(|track| resolve_grid_track(track, font_size, environment, property))
            .collect(),
        Some(_) => Err(invalid(property)),
        None => Ok(Vec::new()),
    }
}

pub(super) fn resolve_grid_track(
    value: &GridTrackSizingValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedGridTrackSizing, StyleResolutionError> {
    let min =
        match &value.min {
            GridMinTrackSizingValue::Fixed(value) => ComputedGridMinTrackSizing::Fixed(
                resolve_affine(value, font_size, environment, property)?,
            ),
            GridMinTrackSizingValue::MinContent => ComputedGridMinTrackSizing::MinContent,
            GridMinTrackSizingValue::MaxContent => ComputedGridMinTrackSizing::MaxContent,
            GridMinTrackSizingValue::Auto => ComputedGridMinTrackSizing::Auto,
        };
    let max =
        match &value.max {
            GridMaxTrackSizingValue::Fixed(value) => ComputedGridMaxTrackSizing::Fixed(
                resolve_affine(value, font_size, environment, property)?,
            ),
            GridMaxTrackSizingValue::MinContent => ComputedGridMaxTrackSizing::MinContent,
            GridMaxTrackSizingValue::MaxContent => ComputedGridMaxTrackSizing::MaxContent,
            GridMaxTrackSizingValue::FitContent(value) => ComputedGridMaxTrackSizing::FitContent(
                resolve_affine(value, font_size, environment, property)?,
            ),
            GridMaxTrackSizingValue::Auto => ComputedGridMaxTrackSizing::Auto,
            GridMaxTrackSizingValue::Fraction(value)
                if value.get().is_finite() && value.get() >= 0.0 =>
            {
                ComputedGridMaxTrackSizing::Fraction(*value)
            }
            GridMaxTrackSizingValue::Fraction(_) => return Err(invalid(property)),
        };
    Ok(ComputedGridTrackSizing { min, max })
}

pub(super) fn resolve_grid_placement(
    value: Option<&StyleValue>,
    property: StyleProperty,
) -> Result<GridPlacementValue, StyleResolutionError> {
    match value {
        Some(StyleValue::GridPlacement(GridPlacementValue::Line(0)))
        | Some(StyleValue::GridPlacement(GridPlacementValue::Span(0))) => Err(invalid(property)),
        Some(StyleValue::GridPlacement(GridPlacementValue::NamedLine(name, _)))
        | Some(StyleValue::GridPlacement(GridPlacementValue::NamedSpan(name, _)))
            if name.is_empty() || name.chars().any(char::is_whitespace) =>
        {
            Err(invalid(property))
        }
        Some(StyleValue::GridPlacement(value)) => Ok(value.clone()),
        Some(_) => Err(invalid(property)),
        None => Ok(GridPlacementValue::Auto),
    }
}

pub(super) fn validate_grid_template_areas(
    value: &GridTemplateAreasValue,
) -> Result<(), StyleResolutionError> {
    let area_shapes_are_valid = value.row_count > 0
        && value.column_count > 0
        && value.areas.iter().all(|area| {
            !area.name.is_empty()
                && !area.name.chars().any(char::is_whitespace)
                && area.row_start < area.row_end
                && area.row_end <= value.row_count
                && area.column_start < area.column_end
                && area.column_end <= value.column_count
        });
    let names_are_unique = value.areas.iter().enumerate().all(|(index, area)| {
        value.areas[index + 1..]
            .iter()
            .all(|other| other.name != area.name)
    });
    let areas_do_not_overlap = value.areas.iter().enumerate().all(|(index, area)| {
        value.areas[index + 1..].iter().all(|other| {
            area.row_end <= other.row_start
                || other.row_end <= area.row_start
                || area.column_end <= other.column_start
                || other.column_end <= area.column_start
        })
    });
    if area_shapes_are_valid && names_are_unique && areas_do_not_overlap {
        Ok(())
    } else {
        Err(invalid(StyleProperty::GridTemplateAreas))
    }
}

pub(super) fn copied<T: Copy>(
    value: Option<&StyleValue>,
    property: StyleProperty,
    convert: impl FnOnce(&StyleValue) -> Option<T>,
) -> Result<Option<T>, StyleResolutionError> {
    value.map_or(Ok(None), |value| {
        convert(value).map(Some).ok_or_else(|| invalid(property))
    })
}

pub(super) fn resolve_optional_size(
    value: Option<&StyleValue>,
    initial: ComputedSizeValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedSizeValue, StyleResolutionError> {
    match value {
        Some(StyleValue::Size(value)) => resolve_size(value, font_size, environment, property),
        Some(_) => Err(invalid(property)),
        None => Ok(initial),
    }
}

pub(super) fn resolve_size(
    value: &SizeValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedSizeValue, StyleResolutionError> {
    Ok(match value {
        SizeValue::Auto => ComputedSizeValue::Auto,
        SizeValue::LengthPercentage(value) => {
            ComputedSizeValue::Value(resolve_affine(value, font_size, environment, property)?)
        }
        SizeValue::MaxContent => ComputedSizeValue::MaxContent,
        SizeValue::MinContent => ComputedSizeValue::MinContent,
        SizeValue::FitContent(limit) => ComputedSizeValue::FitContent(
            limit
                .as_ref()
                .map(|value| resolve_affine(value, font_size, environment, property))
                .transpose()?,
        ),
        SizeValue::None => ComputedSizeValue::None,
    })
}

pub(super) fn resolve_optional_auto(
    value: Option<&StyleValue>,
    initial: ComputedLengthPercentageAuto,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentageAuto, StyleResolutionError> {
    match value {
        Some(StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::Auto)) => {
            Ok(ComputedLengthPercentageAuto::Auto)
        }
        Some(StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(
            value,
        ))) => Ok(ComputedLengthPercentageAuto::Value(resolve_affine(
            value,
            font_size,
            environment,
            property,
        )?)),
        Some(_) => Err(invalid(property)),
        None => Ok(initial),
    }
}

pub(super) fn resolve_optional_length_percentage(
    value: Option<&StyleValue>,
    initial: ComputedLengthPercentage,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentage, StyleResolutionError> {
    match value {
        Some(StyleValue::LengthPercentage(value)) => {
            resolve_affine(value, font_size, environment, property)
        }
        Some(StyleValue::Length(value)) => resolve_affine(
            &LengthPercentageValue::Length(*value),
            font_size,
            environment,
            property,
        ),
        Some(_) => Err(invalid(property)),
        None => Ok(initial),
    }
}

pub(super) fn resolve_non_negative_length_percentage(
    value: Option<&StyleValue>,
    initial: ComputedLengthPercentage,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentage, StyleResolutionError> {
    let resolved =
        resolve_optional_length_percentage(value, initial, font_size, environment, property)?;
    if resolved.length() < 0.0 || resolved.fraction() < 0.0 {
        Err(invalid(property))
    } else {
        Ok(resolved)
    }
}

pub(super) fn resolve_insets(
    specified: &SpecifiedStyle,
    direction: DirectionValue,
    font_size: f32,
    environment: StyleEnvironment,
) -> Result<Edges<ComputedLengthPercentageAuto>, StyleResolutionError> {
    let mut inset = Edges::all(ComputedLengthPercentageAuto::Auto);
    for declaration in specified.resolved() {
        let (edge, property) = match declaration.property() {
            StyleProperty::Top => (&mut inset.top, StyleProperty::Top),
            StyleProperty::Right => (&mut inset.right, StyleProperty::Right),
            StyleProperty::Bottom => (&mut inset.bottom, StyleProperty::Bottom),
            StyleProperty::Left => (&mut inset.left, StyleProperty::Left),
            StyleProperty::InsetInlineStart if direction == DirectionValue::Ltr => {
                (&mut inset.left, StyleProperty::InsetInlineStart)
            }
            StyleProperty::InsetInlineStart => (&mut inset.right, StyleProperty::InsetInlineStart),
            StyleProperty::InsetInlineEnd if direction == DirectionValue::Ltr => {
                (&mut inset.right, StyleProperty::InsetInlineEnd)
            }
            StyleProperty::InsetInlineEnd => (&mut inset.left, StyleProperty::InsetInlineEnd),
            _ => continue,
        };
        *edge = match declaration.value() {
            StyleValue::LengthPercentage(value) => ComputedLengthPercentageAuto::Value(
                resolve_affine(value, font_size, environment, property)?,
            ),
            StyleValue::Length(value) => ComputedLengthPercentageAuto::Value(resolve_affine(
                &LengthPercentageValue::Length(*value),
                font_size,
                environment,
                property,
            )?),
            _ => return Err(invalid(property)),
        };
    }
    Ok(inset)
}

pub(super) fn resolve_borders(
    specified: &SpecifiedStyle,
    direction: DirectionValue,
    font_size: f32,
    environment: StyleEnvironment,
) -> Result<Edges<ComputedLengthPercentage>, StyleResolutionError> {
    let mut border = Edges::all(ComputedLengthPercentage::ZERO);
    for declaration in specified.resolved() {
        let (edge, property) = match declaration.property() {
            StyleProperty::BorderTopWidth => (&mut border.top, StyleProperty::BorderTopWidth),
            StyleProperty::BorderRightWidth => (&mut border.right, StyleProperty::BorderRightWidth),
            StyleProperty::BorderBottomWidth => {
                (&mut border.bottom, StyleProperty::BorderBottomWidth)
            }
            StyleProperty::BorderLeftWidth => (&mut border.left, StyleProperty::BorderLeftWidth),
            StyleProperty::BorderInlineStartWidth if direction == DirectionValue::Ltr => {
                (&mut border.left, StyleProperty::BorderInlineStartWidth)
            }
            StyleProperty::BorderInlineStartWidth => {
                (&mut border.right, StyleProperty::BorderInlineStartWidth)
            }
            StyleProperty::BorderInlineEndWidth if direction == DirectionValue::Ltr => {
                (&mut border.right, StyleProperty::BorderInlineEndWidth)
            }
            StyleProperty::BorderInlineEndWidth => {
                (&mut border.left, StyleProperty::BorderInlineEndWidth)
            }
            _ => continue,
        };
        *edge = resolve_non_negative_length_percentage(
            Some(declaration.value()),
            *edge,
            font_size,
            environment,
            property,
        )?;
    }
    Ok(border)
}

pub(super) fn resolve_non_negative_number(
    value: Option<&StyleValue>,
    initial: StyleNumber,
    property: StyleProperty,
) -> Result<StyleNumber, StyleResolutionError> {
    match value {
        Some(StyleValue::Number(value)) if value.get().is_finite() => {
            Ok(StyleNumber::new(value.get().max(0.0)))
        }
        Some(_) => Err(invalid(property)),
        None => Ok(initial),
    }
}

pub(super) fn resolve_aspect_ratio(value: AspectRatioValue) -> Result<f32, StyleResolutionError> {
    if !value.width().is_finite()
        || !value.height().is_finite()
        || value.width() <= 0.0
        || value.height() <= 0.0
    {
        return Err(invalid(StyleProperty::AspectRatio));
    }
    let ratio = value.width() / value.height();
    if ratio.is_finite() {
        Ok(ratio)
    } else {
        Err(invalid(StyleProperty::AspectRatio))
    }
}

pub(crate) fn resolve_affine(
    value: &LengthPercentageValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<ComputedLengthPercentage, StyleResolutionError> {
    let affine = match value {
        LengthPercentageValue::Length(value) => Affine::new(
            resolve_absolute(*value, font_size, environment, property)?,
            0.0,
        ),
        LengthPercentageValue::Percentage(value) => {
            Affine::new(0.0, finite(*value, property)? / 100.0)
        }
        LengthPercentageValue::Calc(value) => {
            match evaluate_affine_calc(value, font_size, environment, property)? {
                CalcQuantity::Affine(value) => value,
                CalcQuantity::Scalar(_) => {
                    return Err(StyleResolutionError::InvalidCalculation(property));
                }
            }
        }
    };
    if affine.length.is_finite() && affine.fraction.is_finite() {
        Ok(ComputedLengthPercentage::new(
            affine.length,
            affine.fraction,
        ))
    } else {
        Err(invalid(property))
    }
}

pub(super) fn resolve_absolute(
    value: LengthValue,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    let (number, multiplier) = match value {
        LengthValue::Zero => return Ok(0.0),
        LengthValue::Dimension { value, unit } => {
            let multiplier = match unit {
                LengthUnit::Px => 1.0,
                LengthUnit::Rpx => environment.viewport_width() / RPX_REFERENCE_WIDTH,
                LengthUnit::Ppx => 1.0 / environment.scale_factor(),
                LengthUnit::Em => font_size,
                LengthUnit::Rem => environment.root_font_size(),
                LengthUnit::Vh => environment.viewport_height() / 100.0,
                LengthUnit::Vw => environment.viewport_width() / 100.0,
            };
            (finite(value, property)?, multiplier)
        }
    };
    let result = number * multiplier;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(invalid(property))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Affine {
    length: f32,
    fraction: f32,
}

impl Affine {
    const fn new(length: f32, fraction: f32) -> Self {
        Self { length, fraction }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.length + other.length, self.fraction + other.fraction)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.length - other.length, self.fraction - other.fraction)
    }

    fn scale(self, scalar: f32) -> Self {
        Self::new(self.length * scalar, self.fraction * scalar)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CalcQuantity {
    Scalar(f32),
    Affine(Affine),
}

pub(super) fn evaluate_affine_calc(
    value: &CalcExpression,
    font_size: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<CalcQuantity, StyleResolutionError> {
    let invalid_calc = || StyleResolutionError::InvalidCalculation(property);
    match value {
        CalcExpression::Value(value) => resolve_affine(value, font_size, environment, property)
            .map(|value| CalcQuantity::Affine(Affine::new(value.length(), value.fraction()))),
        CalcExpression::Number(value) => finite(*value, property).map(CalcQuantity::Scalar),
        CalcExpression::Variable(_) => Err(invalid_calc()),
        CalcExpression::Add(left, right) => match (
            evaluate_affine_calc(left, font_size, environment, property)?,
            evaluate_affine_calc(right, font_size, environment, property)?,
        ) {
            (CalcQuantity::Scalar(left), CalcQuantity::Scalar(right)) => {
                Ok(CalcQuantity::Scalar(left + right))
            }
            (CalcQuantity::Affine(left), CalcQuantity::Affine(right)) => {
                Ok(CalcQuantity::Affine(left.add(right)))
            }
            _ => Err(invalid_calc()),
        },
        CalcExpression::Sub(left, right) => match (
            evaluate_affine_calc(left, font_size, environment, property)?,
            evaluate_affine_calc(right, font_size, environment, property)?,
        ) {
            (CalcQuantity::Scalar(left), CalcQuantity::Scalar(right)) => {
                Ok(CalcQuantity::Scalar(left - right))
            }
            (CalcQuantity::Affine(left), CalcQuantity::Affine(right)) => {
                Ok(CalcQuantity::Affine(left.sub(right)))
            }
            _ => Err(invalid_calc()),
        },
        CalcExpression::Mul(left, right) => match (
            evaluate_affine_calc(left, font_size, environment, property)?,
            evaluate_affine_calc(right, font_size, environment, property)?,
        ) {
            (CalcQuantity::Scalar(left), CalcQuantity::Scalar(right)) => {
                Ok(CalcQuantity::Scalar(left * right))
            }
            (CalcQuantity::Scalar(scalar), CalcQuantity::Affine(affine))
            | (CalcQuantity::Affine(affine), CalcQuantity::Scalar(scalar)) => {
                Ok(CalcQuantity::Affine(affine.scale(scalar)))
            }
            (CalcQuantity::Affine(_), CalcQuantity::Affine(_)) => Err(invalid_calc()),
        },
        CalcExpression::Div(left, right) => {
            let left = evaluate_affine_calc(left, font_size, environment, property)?;
            let right = evaluate_affine_calc(right, font_size, environment, property)?;
            match (left, right) {
                (_, CalcQuantity::Scalar(0.0)) => Err(invalid_calc()),
                (CalcQuantity::Scalar(left), CalcQuantity::Scalar(right)) => {
                    Ok(CalcQuantity::Scalar(left / right))
                }
                (CalcQuantity::Affine(left), CalcQuantity::Scalar(right)) => {
                    Ok(CalcQuantity::Affine(left.scale(1.0 / right)))
                }
                (_, CalcQuantity::Affine(_)) => Err(invalid_calc()),
            }
        }
    }
}

pub(super) fn finite(
    value: StyleNumber,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    if value.get().is_finite() {
        Ok(value.get())
    } else {
        Err(invalid(property))
    }
}

pub(super) fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

#[derive(Default)]
pub(super) struct LayoutDeclarations<'a> {
    pub(super) display: Option<&'a StyleValue>,
    pub(super) float: Option<&'a StyleValue>,
    pub(super) clear: Option<&'a StyleValue>,
    pub(super) overflow_x: Option<&'a StyleValue>,
    pub(super) overflow_y: Option<&'a StyleValue>,
    pub(super) position: Option<&'a StyleValue>,
    pub(super) direction: Option<&'a StyleValue>,
    pub(super) box_sizing: Option<&'a StyleValue>,
    pub(super) width: Option<&'a StyleValue>,
    pub(super) height: Option<&'a StyleValue>,
    pub(super) min_width: Option<&'a StyleValue>,
    pub(super) min_height: Option<&'a StyleValue>,
    pub(super) max_width: Option<&'a StyleValue>,
    pub(super) max_height: Option<&'a StyleValue>,
    pub(super) margin_top: Option<&'a StyleValue>,
    pub(super) margin_right: Option<&'a StyleValue>,
    pub(super) margin_bottom: Option<&'a StyleValue>,
    pub(super) margin_left: Option<&'a StyleValue>,
    pub(super) padding_top: Option<&'a StyleValue>,
    pub(super) padding_right: Option<&'a StyleValue>,
    pub(super) padding_bottom: Option<&'a StyleValue>,
    pub(super) padding_left: Option<&'a StyleValue>,
    pub(super) flex_direction: Option<&'a StyleValue>,
    pub(super) flex_wrap: Option<&'a StyleValue>,
    pub(super) flex_grow: Option<&'a StyleValue>,
    pub(super) flex_shrink: Option<&'a StyleValue>,
    pub(super) flex_basis: Option<&'a StyleValue>,
    pub(super) justify_content: Option<&'a StyleValue>,
    pub(super) align_items: Option<&'a StyleValue>,
    pub(super) align_self: Option<&'a StyleValue>,
    pub(super) justify_items: Option<&'a StyleValue>,
    pub(super) justify_self: Option<&'a StyleValue>,
    pub(super) align_content: Option<&'a StyleValue>,
    pub(super) row_gap: Option<&'a StyleValue>,
    pub(super) column_gap: Option<&'a StyleValue>,
    pub(super) aspect_ratio: Option<&'a StyleValue>,
    pub(super) order: Option<&'a StyleValue>,
    pub(super) grid_template_columns: Option<&'a StyleValue>,
    pub(super) grid_template_rows: Option<&'a StyleValue>,
    pub(super) grid_auto_columns: Option<&'a StyleValue>,
    pub(super) grid_auto_rows: Option<&'a StyleValue>,
    pub(super) grid_auto_flow: Option<&'a StyleValue>,
    pub(super) grid_template_areas: Option<&'a StyleValue>,
    pub(super) grid_column_start: Option<&'a StyleValue>,
    pub(super) grid_column_end: Option<&'a StyleValue>,
    pub(super) grid_row_start: Option<&'a StyleValue>,
    pub(super) grid_row_end: Option<&'a StyleValue>,
}

impl<'a> LayoutDeclarations<'a> {
    pub(super) fn from_specified(specified: &'a SpecifiedStyle) -> Self {
        let mut values = Self::default();
        for declaration in specified.resolved() {
            let slot = match declaration.property() {
                StyleProperty::Display => &mut values.display,
                StyleProperty::Float => &mut values.float,
                StyleProperty::Clear => &mut values.clear,
                StyleProperty::OverflowX => &mut values.overflow_x,
                StyleProperty::OverflowY => &mut values.overflow_y,
                StyleProperty::Position => &mut values.position,
                StyleProperty::Direction => &mut values.direction,
                StyleProperty::BoxSizing => &mut values.box_sizing,
                StyleProperty::Width => &mut values.width,
                StyleProperty::Height => &mut values.height,
                StyleProperty::MinWidth => &mut values.min_width,
                StyleProperty::MinHeight => &mut values.min_height,
                StyleProperty::MaxWidth => &mut values.max_width,
                StyleProperty::MaxHeight => &mut values.max_height,
                StyleProperty::MarginTop => &mut values.margin_top,
                StyleProperty::MarginRight => &mut values.margin_right,
                StyleProperty::MarginBottom => &mut values.margin_bottom,
                StyleProperty::MarginLeft => &mut values.margin_left,
                StyleProperty::PaddingTop => &mut values.padding_top,
                StyleProperty::PaddingRight => &mut values.padding_right,
                StyleProperty::PaddingBottom => &mut values.padding_bottom,
                StyleProperty::PaddingLeft => &mut values.padding_left,
                StyleProperty::FlexDirection => &mut values.flex_direction,
                StyleProperty::FlexWrap => &mut values.flex_wrap,
                StyleProperty::FlexGrow => &mut values.flex_grow,
                StyleProperty::FlexShrink => &mut values.flex_shrink,
                StyleProperty::FlexBasis => &mut values.flex_basis,
                StyleProperty::JustifyContent => &mut values.justify_content,
                StyleProperty::AlignItems => &mut values.align_items,
                StyleProperty::AlignSelf => &mut values.align_self,
                StyleProperty::JustifyItems => &mut values.justify_items,
                StyleProperty::JustifySelf => &mut values.justify_self,
                StyleProperty::AlignContent => &mut values.align_content,
                StyleProperty::RowGap => &mut values.row_gap,
                StyleProperty::ColumnGap => &mut values.column_gap,
                StyleProperty::AspectRatio => &mut values.aspect_ratio,
                StyleProperty::Order => &mut values.order,
                StyleProperty::GridTemplateColumns => &mut values.grid_template_columns,
                StyleProperty::GridTemplateRows => &mut values.grid_template_rows,
                StyleProperty::GridAutoColumns => &mut values.grid_auto_columns,
                StyleProperty::GridAutoRows => &mut values.grid_auto_rows,
                StyleProperty::GridAutoFlow => &mut values.grid_auto_flow,
                StyleProperty::GridTemplateAreas => &mut values.grid_template_areas,
                StyleProperty::GridColumnStart => &mut values.grid_column_start,
                StyleProperty::GridColumnEnd => &mut values.grid_column_end,
                StyleProperty::GridRowStart => &mut values.grid_row_start,
                StyleProperty::GridRowEnd => &mut values.grid_row_end,
                _ => continue,
            };
            *slot = Some(declaration.value());
        }
        values
    }
}
