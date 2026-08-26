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

const RPX_REFERENCE_WIDTH: f32 = 750.0;

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
            display: DisplayValue::Linear,
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

fn resolve_optional_grid_template(
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

fn resolve_grid_template(
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

fn resolve_optional_grid_tracks(
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

fn resolve_grid_track(
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

fn resolve_grid_placement(
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

fn validate_grid_template_areas(
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

fn copied<T: Copy>(
    value: Option<&StyleValue>,
    property: StyleProperty,
    convert: impl FnOnce(&StyleValue) -> Option<T>,
) -> Result<Option<T>, StyleResolutionError> {
    value.map_or(Ok(None), |value| {
        convert(value).map(Some).ok_or_else(|| invalid(property))
    })
}

fn resolve_optional_size(
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

fn resolve_size(
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

fn resolve_optional_auto(
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

fn resolve_optional_length_percentage(
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

fn resolve_non_negative_length_percentage(
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

fn resolve_insets(
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

fn resolve_borders(
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

fn resolve_non_negative_number(
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

fn resolve_aspect_ratio(value: AspectRatioValue) -> Result<f32, StyleResolutionError> {
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

fn resolve_absolute(
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
struct Affine {
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
enum CalcQuantity {
    Scalar(f32),
    Affine(Affine),
}

fn evaluate_affine_calc(
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

fn finite(value: StyleNumber, property: StyleProperty) -> Result<f32, StyleResolutionError> {
    if value.get().is_finite() {
        Ok(value.get())
    } else {
        Err(invalid(property))
    }
}

fn invalid(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

#[derive(Default)]
struct LayoutDeclarations<'a> {
    display: Option<&'a StyleValue>,
    float: Option<&'a StyleValue>,
    clear: Option<&'a StyleValue>,
    overflow_x: Option<&'a StyleValue>,
    overflow_y: Option<&'a StyleValue>,
    position: Option<&'a StyleValue>,
    direction: Option<&'a StyleValue>,
    box_sizing: Option<&'a StyleValue>,
    width: Option<&'a StyleValue>,
    height: Option<&'a StyleValue>,
    min_width: Option<&'a StyleValue>,
    min_height: Option<&'a StyleValue>,
    max_width: Option<&'a StyleValue>,
    max_height: Option<&'a StyleValue>,
    margin_top: Option<&'a StyleValue>,
    margin_right: Option<&'a StyleValue>,
    margin_bottom: Option<&'a StyleValue>,
    margin_left: Option<&'a StyleValue>,
    padding_top: Option<&'a StyleValue>,
    padding_right: Option<&'a StyleValue>,
    padding_bottom: Option<&'a StyleValue>,
    padding_left: Option<&'a StyleValue>,
    flex_direction: Option<&'a StyleValue>,
    flex_wrap: Option<&'a StyleValue>,
    flex_grow: Option<&'a StyleValue>,
    flex_shrink: Option<&'a StyleValue>,
    flex_basis: Option<&'a StyleValue>,
    justify_content: Option<&'a StyleValue>,
    align_items: Option<&'a StyleValue>,
    align_self: Option<&'a StyleValue>,
    justify_items: Option<&'a StyleValue>,
    justify_self: Option<&'a StyleValue>,
    align_content: Option<&'a StyleValue>,
    row_gap: Option<&'a StyleValue>,
    column_gap: Option<&'a StyleValue>,
    aspect_ratio: Option<&'a StyleValue>,
    order: Option<&'a StyleValue>,
    grid_template_columns: Option<&'a StyleValue>,
    grid_template_rows: Option<&'a StyleValue>,
    grid_auto_columns: Option<&'a StyleValue>,
    grid_auto_rows: Option<&'a StyleValue>,
    grid_auto_flow: Option<&'a StyleValue>,
    grid_template_areas: Option<&'a StyleValue>,
    grid_column_start: Option<&'a StyleValue>,
    grid_column_end: Option<&'a StyleValue>,
    grid_row_start: Option<&'a StyleValue>,
    grid_row_end: Option<&'a StyleValue>,
}

impl<'a> LayoutDeclarations<'a> {
    fn from_specified(specified: &'a SpecifiedStyle) -> Self {
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

#[cfg(test)]
mod tests {
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
        assert_eq!(style.display, DisplayValue::Linear);
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
        let area =
            |name: &str, row_start, row_end, column_start, column_end| GridTemplateAreaValue {
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
                StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(
                    2.0,
                ))),
            )
            .push(
                StyleProperty::MarginBottom,
                StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(
                    percent(3.0),
                )),
            )
            .push(
                StyleProperty::MarginLeft,
                StyleValue::LengthPercentageAuto(LengthPercentageAutoValue::LengthPercentage(px(
                    -4.0,
                ))),
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
                StyleValue::LengthPercentage(percent(3.0)),
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
        assert_eq!(style.border.bottom.fraction(), 0.03);
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
    fn relative_units_become_logical_pixel_components() {
        let environment = StyleEnvironment::new(750.0, 400.0, 2.0, 10.0);
        let cases = [
            (LengthValue::Zero, 0.0),
            (length(2.0, LengthUnit::Px), 2.0),
            (length(2.0, LengthUnit::Rpx), 2.0),
            (length(4.0, LengthUnit::Ppx), 2.0),
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
}
