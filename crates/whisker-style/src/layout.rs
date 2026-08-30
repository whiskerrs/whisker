//! Computed box and flex layout values, independent of any layout library.

use crate::{
    AlignContentValue, AlignItemsValue, AlignSelfValue, AspectRatioValue, BoxSizingValue,
    CalcExpression, ClearValue, DirectionValue, DisplayValue, FlexBasisValue, FlexDirectionValue,
    FlexWrapValue, FloatValue, GridMaxTrackSizingValue, GridMinTrackSizingValue,
    GridTemplateComponentValue, GridTemplateValue, GridTrackSizingValue, JustifyContentValue,
    LengthPercentageAutoValue, LengthPercentageValue, LengthUnit, LengthValue, OverflowValue,
    PositionValue, PropertyImpactSet, SizeValue, SpecifiedStyle, StyleEnvironment, StyleNumber,
    StyleProperty, StyleResolutionError, StyleValue,
};

mod resolution;

#[cfg(test)]
use resolution::evaluate_affine_calc;
pub(crate) use resolution::resolve_affine;
use resolution::*;

/// An affine containing-block-relative value: logical pixels plus a fraction.
///
/// `10px + 25%` is stored as `length = 10` and `fraction = 0.25`. This keeps
/// percentage semantics intact until the layout engine knows the containing
/// block size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedLengthPercentage {
    length: StyleNumber,
    fraction: StyleNumber,
}

impl Default for ComputedLengthPercentage {
    fn default() -> Self {
        Self::ZERO
    }
}

impl ComputedLengthPercentage {
    /// Zero length and zero percentage.
    pub const ZERO: Self = Self::new(0.0, 0.0);

    /// Creates an affine value from logical pixels and a fraction, where `1.0`
    /// means 100 percent. Negative and greater-than-one fractions are valid.
    pub const fn new(length: f32, fraction: f32) -> Self {
        Self {
            length: StyleNumber::new(length),
            fraction: StyleNumber::new(fraction),
        }
    }

    /// Returns the absolute logical-pixel component.
    pub const fn length(self) -> f32 {
        self.length.get()
    }

    /// Returns the containing-block fraction, where `1.0` means 100 percent.
    pub const fn fraction(self) -> f32 {
        self.fraction.get()
    }
}

/// A computed margin or inset value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedLengthPercentageAuto {
    /// Let the layout algorithm choose.
    Auto,
    /// An affine length-percentage value.
    Value(ComputedLengthPercentage),
}

/// A computed size constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedSizeValue {
    /// Let the layout algorithm choose.
    Auto,
    /// An affine length-percentage value.
    Value(ComputedLengthPercentage),
    /// Maximum intrinsic content size.
    MaxContent,
    /// Minimum intrinsic content size.
    MinContent,
    /// Fit intrinsic content, optionally capped.
    FitContent(Option<ComputedLengthPercentage>),
    /// No maximum constraint.
    None,
}

/// A computed flex basis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedFlexBasis {
    /// Use the item's main size.
    Auto,
    /// Use intrinsic content size.
    Content,
    /// Use an explicit affine basis.
    Value(ComputedLengthPercentage),
}

/// Four physical edges in top, right, bottom, left order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edges<T> {
    /// Top edge.
    pub top: T,
    /// Right edge.
    pub right: T,
    /// Bottom edge.
    pub bottom: T,
    /// Left edge.
    pub left: T,
}

impl<T: Copy> Edges<T> {
    const fn all(value: T) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

/// Horizontal and vertical values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Axes<T> {
    /// Horizontal value.
    pub width: T,
    /// Vertical value.
    pub height: T,
}

/// Minimum sizing function for one computed CSS Grid track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedGridMinTrackSizing {
    /// A fixed length or percentage.
    Fixed(ComputedLengthPercentage),
    /// The track's min-content contribution.
    MinContent,
    /// The track's max-content contribution.
    MaxContent,
    /// Automatic minimum sizing.
    Auto,
}

/// Maximum sizing function for one computed CSS Grid track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedGridMaxTrackSizing {
    /// A fixed length or percentage.
    Fixed(ComputedLengthPercentage),
    /// The track's min-content contribution.
    MinContent,
    /// The track's max-content contribution.
    MaxContent,
    /// Fit content up to the supplied limit.
    FitContent(ComputedLengthPercentage),
    /// Automatic maximum sizing.
    Auto,
    /// A flexible `fr` share.
    Fraction(StyleNumber),
}

/// Computed minimum and maximum sizing functions for one CSS Grid track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComputedGridTrackSizing {
    /// Minimum sizing function.
    pub min: ComputedGridMinTrackSizing,
    /// Maximum sizing function.
    pub max: ComputedGridMaxTrackSizing,
}

impl ComputedGridTrackSizing {
    /// Creates a fixed logical-pixel track.
    pub const fn length(value: f32) -> Self {
        let value = ComputedLengthPercentage::new(value, 0.0);
        Self {
            min: ComputedGridMinTrackSizing::Fixed(value),
            max: ComputedGridMaxTrackSizing::Fixed(value),
        }
    }

    /// Creates a flexible `fr` track with an automatic minimum.
    pub const fn fraction(value: f32) -> Self {
        Self {
            min: ComputedGridMinTrackSizing::Auto,
            max: ComputedGridMaxTrackSizing::Fraction(StyleNumber::new(value)),
        }
    }

    /// Creates an automatic track.
    pub const fn auto() -> Self {
        Self {
            min: ComputedGridMinTrackSizing::Auto,
            max: ComputedGridMaxTrackSizing::Auto,
        }
    }
}

/// Repetition count used by `repeat()` in a computed Grid template.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GridRepetitionCountValue {
    /// Repeat a fixed number of times.
    Count(u16),
    /// Add tracks while they fit.
    AutoFill,
    /// Add tracks while they fit and collapse empty tracks.
    AutoFit,
}

/// One repeated fragment in a computed Grid template.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedGridTemplateRepetition {
    /// Repetition count.
    pub count: GridRepetitionCountValue,
    /// Track sizing functions inside the repeated fragment.
    pub tracks: Vec<ComputedGridTrackSizing>,
    /// Named lines surrounding the repeated tracks.
    pub line_names: Vec<Vec<String>>,
}

/// One component of a computed Grid template.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComputedGridTemplateComponent {
    /// One non-repeated track.
    Track(ComputedGridTrackSizing),
    /// A `repeat()` fragment.
    Repeat(ComputedGridTemplateRepetition),
}

/// Computed track components and named lines for one Grid axis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ComputedGridTemplate {
    /// Track or repetition components.
    pub components: Vec<ComputedGridTemplateComponent>,
    /// Named lines outside repetition components.
    pub line_names: Vec<Vec<String>>,
}

impl ComputedGridTemplate {
    /// Creates a template containing only non-repeated tracks.
    pub fn tracks(tracks: impl IntoIterator<Item = ComputedGridTrackSizing>) -> Self {
        let components = tracks
            .into_iter()
            .map(ComputedGridTemplateComponent::Track)
            .collect::<Vec<_>>();
        Self {
            line_names: vec![Vec::new(); components.len() + 1],
            components,
        }
    }
}

/// Placement of one edge of a Grid item.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridPlacementValue {
    /// Use auto-placement.
    #[default]
    Auto,
    /// Place at a numbered Grid line.
    Line(i16),
    /// Place at the nth line with this name.
    NamedLine(String, i16),
    /// Span a number of tracks.
    Span(u16),
    /// Span to the nth line with this name.
    NamedSpan(String, u16),
}

/// Start and end placement for one Grid item axis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GridPlacementLineValue {
    /// Start edge placement.
    pub start: GridPlacementValue,
    /// End edge placement.
    pub end: GridPlacementValue,
}

impl GridPlacementLineValue {
    /// Places an item between two numbered lines.
    pub const fn lines(start: i16, end: i16) -> Self {
        Self {
            start: GridPlacementValue::Line(start),
            end: GridPlacementValue::Line(end),
        }
    }
}

/// Auto-placement direction and packing mode for Grid items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridAutoFlowValue {
    /// Fill rows using sparse placement.
    #[default]
    Row,
    /// Fill columns using sparse placement.
    Column,
    /// Fill rows and back-fill holes.
    RowDense,
    /// Fill columns and back-fill holes.
    ColumnDense,
}

/// One named rectangle in a Grid area template.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTemplateAreaValue {
    /// Area name.
    pub name: String,
    /// Zero-based starting row.
    pub row_start: u16,
    /// Exclusive ending row.
    pub row_end: u16,
    /// Zero-based starting column.
    pub column_start: u16,
    /// Exclusive ending column.
    pub column_end: u16,
}

/// Named area rectangles and dimensions for `grid-template-areas`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTemplateAreasValue {
    /// Named rectangles.
    pub areas: Vec<GridTemplateAreaValue>,
    /// Number of template rows.
    pub row_count: u16,
    /// Number of template columns.
    pub column_count: u16,
}

impl<T: Copy> Axes<T> {
    const fn all(value: T) -> Self {
        Self {
            width: value,
            height: value,
        }
    }
}

/// Taffy-independent computed box and flex layout input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedLayoutStyle {
    /// Selected layout algorithm.
    pub display: DisplayValue,
    /// Float side when this node participates in block layout.
    pub float: FloatValue,
    /// Clearance applied against preceding floats.
    pub clear: ClearValue,
    /// Horizontal and vertical overflow behavior that also affects Taffy's automatic minimums.
    pub overflow: Axes<OverflowValue>,
    /// Positioning model.
    pub position: PositionValue,
    /// Inline writing direction.
    pub direction: DirectionValue,
    /// Declared box sizing model.
    pub box_sizing: BoxSizingValue,
    /// Preferred width and height.
    pub size: Axes<ComputedSizeValue>,
    /// Minimum width and height.
    pub min_size: Axes<ComputedSizeValue>,
    /// Maximum width and height.
    pub max_size: Axes<ComputedSizeValue>,
    /// Physical margins.
    pub margin: Edges<ComputedLengthPercentageAuto>,
    /// Physical padding. Negative results are clamped by `whisker-layout`.
    pub padding: Edges<ComputedLengthPercentage>,
    /// Physical border widths. Negative results are rejected during resolution.
    pub border: Edges<ComputedLengthPercentage>,
    /// Physical position offsets after resolving logical inline edges.
    pub inset: Edges<ComputedLengthPercentageAuto>,
    /// Main-axis flex direction.
    pub flex_direction: FlexDirectionValue,
    /// Flex line wrapping.
    pub flex_wrap: FlexWrapValue,
    /// Non-negative flex growth factor.
    pub flex_grow: StyleNumber,
    /// Non-negative flex shrink factor.
    pub flex_shrink: StyleNumber,
    /// Main-axis flex basis.
    pub flex_basis: ComputedFlexBasis,
    /// Main-axis distribution.
    pub justify_content: JustifyContentValue,
    /// Cross-axis child alignment.
    pub align_items: AlignItemsValue,
    /// Per-item cross-axis alignment.
    pub align_self: AlignSelfValue,
    /// Inline-axis child alignment for Grid containers.
    pub justify_items: Option<AlignItemsValue>,
    /// Inline-axis alignment for one Grid item.
    pub justify_self: Option<AlignSelfValue>,
    /// Wrapped-line cross-axis distribution.
    pub align_content: AlignContentValue,
    /// Row and column gaps.
    pub gap: Axes<ComputedLengthPercentage>,
    /// Optional computed width-to-height ratio.
    pub aspect_ratio: Option<StyleNumber>,
    /// Flex/grid ordering key.
    pub order: i32,
    /// Explicit Grid column template.
    pub grid_template_columns: ComputedGridTemplate,
    /// Explicit Grid row template.
    pub grid_template_rows: ComputedGridTemplate,
    /// Implicit Grid column sizing functions.
    pub grid_auto_columns: Vec<ComputedGridTrackSizing>,
    /// Implicit Grid row sizing functions.
    pub grid_auto_rows: Vec<ComputedGridTrackSizing>,
    /// Grid auto-placement mode.
    pub grid_auto_flow: GridAutoFlowValue,
    /// Optional named area template.
    pub grid_template_areas: Option<GridTemplateAreasValue>,
    /// Grid column placement for this item.
    pub grid_column: GridPlacementLineValue,
    /// Grid row placement for this item.
    pub grid_row: GridPlacementLineValue,
}

impl Default for ComputedLayoutStyle {
    fn default() -> Self {
        Self {
            display: DisplayValue::Flex,
            float: FloatValue::None,
            clear: ClearValue::None,
            overflow: Axes::all(OverflowValue::Visible),
            position: PositionValue::Relative,
            direction: DirectionValue::Ltr,
            box_sizing: BoxSizingValue::BorderBox,
            size: Axes::all(ComputedSizeValue::Auto),
            min_size: Axes::all(ComputedSizeValue::Auto),
            max_size: Axes::all(ComputedSizeValue::None),
            margin: Edges::all(ComputedLengthPercentageAuto::Value(
                ComputedLengthPercentage::ZERO,
            )),
            padding: Edges::all(ComputedLengthPercentage::ZERO),
            border: Edges::all(ComputedLengthPercentage::ZERO),
            inset: Edges::all(ComputedLengthPercentageAuto::Auto),
            flex_direction: FlexDirectionValue::Row,
            flex_wrap: FlexWrapValue::NoWrap,
            flex_grow: StyleNumber::new(0.0),
            flex_shrink: StyleNumber::new(1.0),
            flex_basis: ComputedFlexBasis::Auto,
            justify_content: JustifyContentValue::FlexStart,
            align_items: AlignItemsValue::Stretch,
            align_self: AlignSelfValue::Auto,
            justify_items: None,
            justify_self: None,
            align_content: AlignContentValue::Stretch,
            gap: Axes::all(ComputedLengthPercentage::ZERO),
            aspect_ratio: None,
            order: 0,
            grid_template_columns: ComputedGridTemplate::default(),
            grid_template_rows: ComputedGridTemplate::default(),
            grid_auto_columns: Vec::new(),
            grid_auto_rows: Vec::new(),
            grid_auto_flow: GridAutoFlowValue::Row,
            grid_template_areas: None,
            grid_column: GridPlacementLineValue::default(),
            grid_row: GridPlacementLineValue::default(),
        }
    }
}

impl ComputedLayoutStyle {
    /// Returns layout invalidation when any computed layout input changed.
    pub fn changes_from(&self, previous: &Self) -> PropertyImpactSet {
        if self == previous {
            PropertyImpactSet::EMPTY
        } else {
            PropertyImpactSet::LAYOUT
        }
    }
}

/// Resolves all currently supported box and flex declarations.
pub(crate) fn resolve_layout_style(
    specified: &SpecifiedStyle,
    font_size: f32,
    inherited_direction: DirectionValue,
    environment: StyleEnvironment,
) -> Result<ComputedLayoutStyle, StyleResolutionError> {
    let mut style = ComputedLayoutStyle {
        direction: inherited_direction,
        ..ComputedLayoutStyle::default()
    };
    let declarations = LayoutDeclarations::from_specified(specified);

    style.display = copied(
        declarations.display,
        StyleProperty::Display,
        |value| match value {
            StyleValue::Display(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.display);
    style.float = copied(
        declarations.float,
        StyleProperty::Float,
        |value| match value {
            StyleValue::Float(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.float);
    style.clear = copied(
        declarations.clear,
        StyleProperty::Clear,
        |value| match value {
            StyleValue::Clear(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.clear);
    style.overflow.width =
        copied(
            declarations.overflow_x,
            StyleProperty::OverflowX,
            |value| match value {
                StyleValue::Overflow(value) => Some(*value),
                _ => None,
            },
        )?
        .unwrap_or(style.overflow.width);
    style.overflow.height =
        copied(
            declarations.overflow_y,
            StyleProperty::OverflowY,
            |value| match value {
                StyleValue::Overflow(value) => Some(*value),
                _ => None,
            },
        )?
        .unwrap_or(style.overflow.height);
    style.position = copied(
        declarations.position,
        StyleProperty::Position,
        |value| match value {
            StyleValue::Position(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.position);
    style.direction = copied(
        declarations.direction,
        StyleProperty::Direction,
        |value| match value {
            StyleValue::Direction(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.direction);
    style.box_sizing = copied(
        declarations.box_sizing,
        StyleProperty::BoxSizing,
        |value| match value {
            StyleValue::BoxSizing(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.box_sizing);

    style.size.width = resolve_optional_size(
        declarations.width,
        style.size.width,
        font_size,
        environment,
        StyleProperty::Width,
    )?;
    style.size.height = resolve_optional_size(
        declarations.height,
        style.size.height,
        font_size,
        environment,
        StyleProperty::Height,
    )?;
    style.min_size.width = resolve_optional_size(
        declarations.min_width,
        style.min_size.width,
        font_size,
        environment,
        StyleProperty::MinWidth,
    )?;
    style.min_size.height = resolve_optional_size(
        declarations.min_height,
        style.min_size.height,
        font_size,
        environment,
        StyleProperty::MinHeight,
    )?;
    style.max_size.width = resolve_optional_size(
        declarations.max_width,
        style.max_size.width,
        font_size,
        environment,
        StyleProperty::MaxWidth,
    )?;
    style.max_size.height = resolve_optional_size(
        declarations.max_height,
        style.max_size.height,
        font_size,
        environment,
        StyleProperty::MaxHeight,
    )?;

    style.margin.top = resolve_optional_auto(
        declarations.margin_top,
        style.margin.top,
        font_size,
        environment,
        StyleProperty::MarginTop,
    )?;
    style.margin.right = resolve_optional_auto(
        declarations.margin_right,
        style.margin.right,
        font_size,
        environment,
        StyleProperty::MarginRight,
    )?;
    style.margin.bottom = resolve_optional_auto(
        declarations.margin_bottom,
        style.margin.bottom,
        font_size,
        environment,
        StyleProperty::MarginBottom,
    )?;
    style.margin.left = resolve_optional_auto(
        declarations.margin_left,
        style.margin.left,
        font_size,
        environment,
        StyleProperty::MarginLeft,
    )?;

    style.padding.top = resolve_optional_length_percentage(
        declarations.padding_top,
        style.padding.top,
        font_size,
        environment,
        StyleProperty::PaddingTop,
    )?;
    style.padding.right = resolve_optional_length_percentage(
        declarations.padding_right,
        style.padding.right,
        font_size,
        environment,
        StyleProperty::PaddingRight,
    )?;
    style.padding.bottom = resolve_optional_length_percentage(
        declarations.padding_bottom,
        style.padding.bottom,
        font_size,
        environment,
        StyleProperty::PaddingBottom,
    )?;
    style.padding.left = resolve_optional_length_percentage(
        declarations.padding_left,
        style.padding.left,
        font_size,
        environment,
        StyleProperty::PaddingLeft,
    )?;

    style.border = resolve_borders(specified, style.direction, font_size, environment)?;

    style.inset = resolve_insets(specified, style.direction, font_size, environment)?;

    style.flex_direction = copied(
        declarations.flex_direction,
        StyleProperty::FlexDirection,
        |value| match value {
            StyleValue::FlexDirection(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.flex_direction);
    style.flex_wrap = copied(
        declarations.flex_wrap,
        StyleProperty::FlexWrap,
        |value| match value {
            StyleValue::FlexWrap(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.flex_wrap);
    style.flex_grow = resolve_non_negative_number(
        declarations.flex_grow,
        style.flex_grow,
        StyleProperty::FlexGrow,
    )?;
    style.flex_shrink = resolve_non_negative_number(
        declarations.flex_shrink,
        style.flex_shrink,
        StyleProperty::FlexShrink,
    )?;
    if let Some(value) = declarations.flex_basis {
        match value {
            StyleValue::FlexBasis(FlexBasisValue::Auto) => {
                style.flex_basis = ComputedFlexBasis::Auto;
            }
            StyleValue::FlexBasis(FlexBasisValue::Content) => {
                style.flex_basis = ComputedFlexBasis::Content;
            }
            StyleValue::FlexBasis(FlexBasisValue::LengthPercentage(value)) => {
                style.flex_basis = ComputedFlexBasis::Value(resolve_affine(
                    value,
                    font_size,
                    environment,
                    StyleProperty::FlexBasis,
                )?);
            }
            _ => return Err(invalid(StyleProperty::FlexBasis)),
        }
    }
    style.justify_content = copied(
        declarations.justify_content,
        StyleProperty::JustifyContent,
        |value| match value {
            StyleValue::JustifyContent(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.justify_content);
    style.align_items = copied(
        declarations.align_items,
        StyleProperty::AlignItems,
        |value| match value {
            StyleValue::AlignItems(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.align_items);
    style.align_self = copied(
        declarations.align_self,
        StyleProperty::AlignSelf,
        |value| match value {
            StyleValue::AlignSelf(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.align_self);
    style.justify_items = copied(
        declarations.justify_items,
        StyleProperty::JustifyItems,
        |value| match value {
            StyleValue::AlignItems(value) => Some(*value),
            _ => None,
        },
    )?;
    style.justify_self = match declarations.justify_self {
        Some(StyleValue::AlignSelf(AlignSelfValue::Auto)) | None => None,
        Some(StyleValue::AlignSelf(value)) => Some(*value),
        Some(_) => return Err(invalid(StyleProperty::JustifySelf)),
    };
    style.align_content = copied(
        declarations.align_content,
        StyleProperty::AlignContent,
        |value| match value {
            StyleValue::AlignContent(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.align_content);
    style.gap.height = resolve_optional_length_percentage(
        declarations.row_gap,
        style.gap.height,
        font_size,
        environment,
        StyleProperty::RowGap,
    )?;
    style.gap.width = resolve_optional_length_percentage(
        declarations.column_gap,
        style.gap.width,
        font_size,
        environment,
        StyleProperty::ColumnGap,
    )?;
    if let Some(value) = declarations.aspect_ratio {
        let StyleValue::AspectRatio(value) = value else {
            return Err(invalid(StyleProperty::AspectRatio));
        };
        style.aspect_ratio = Some(StyleNumber::new(resolve_aspect_ratio(*value)?));
    }
    if let Some(value) = declarations.order {
        let StyleValue::Integer(value) = value else {
            return Err(invalid(StyleProperty::Order));
        };
        style.order = i32::try_from(*value).map_err(|_| invalid(StyleProperty::Order))?;
    }
    style.grid_template_columns = resolve_optional_grid_template(
        declarations.grid_template_columns,
        font_size,
        environment,
        StyleProperty::GridTemplateColumns,
    )?;
    style.grid_template_rows = resolve_optional_grid_template(
        declarations.grid_template_rows,
        font_size,
        environment,
        StyleProperty::GridTemplateRows,
    )?;
    style.grid_auto_columns = resolve_optional_grid_tracks(
        declarations.grid_auto_columns,
        font_size,
        environment,
        StyleProperty::GridAutoColumns,
    )?;
    style.grid_auto_rows = resolve_optional_grid_tracks(
        declarations.grid_auto_rows,
        font_size,
        environment,
        StyleProperty::GridAutoRows,
    )?;
    style.grid_auto_flow = copied(
        declarations.grid_auto_flow,
        StyleProperty::GridAutoFlow,
        |value| match value {
            StyleValue::GridAutoFlow(value) => Some(*value),
            _ => None,
        },
    )?
    .unwrap_or(style.grid_auto_flow);
    style.grid_template_areas = match declarations.grid_template_areas {
        Some(StyleValue::GridTemplateAreas(value)) => {
            validate_grid_template_areas(value)?;
            Some(value.clone())
        }
        Some(_) => return Err(invalid(StyleProperty::GridTemplateAreas)),
        None => None,
    };
    style.grid_column.start = resolve_grid_placement(
        declarations.grid_column_start,
        StyleProperty::GridColumnStart,
    )?;
    style.grid_column.end =
        resolve_grid_placement(declarations.grid_column_end, StyleProperty::GridColumnEnd)?;
    style.grid_row.start =
        resolve_grid_placement(declarations.grid_row_start, StyleProperty::GridRowStart)?;
    style.grid_row.end =
        resolve_grid_placement(declarations.grid_row_end, StyleProperty::GridRowEnd)?;

    Ok(style)
}

#[cfg(test)]
mod tests;
