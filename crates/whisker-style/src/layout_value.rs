//! Renderer-independent specified values for box and flex layout.

use crate::{LengthPercentageValue, StyleNumber};

/// The layout algorithm selected for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayValue {
    /// Remove the node from layout.
    None,
    /// CSS flexbox.
    Flex,
    /// CSS grid.
    Grid,
    /// CSS block layout.
    Block,
    /// CSS block layout establishing a new block formatting context.
    FlowRoot,
    /// Lynx-compatible linear layout.
    Linear,
    /// Lynx-compatible relative layout.
    Relative,
}

/// Side to which a box floats in a block formatting context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FloatValue {
    /// Do not float the box.
    #[default]
    None,
    /// Float to the physical left side.
    Left,
    /// Float to the physical right side.
    Right,
}

/// Floats that a block box must clear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClearValue {
    /// Do not add clearance.
    #[default]
    None,
    /// Clear preceding left floats.
    Left,
    /// Clear preceding right floats.
    Right,
    /// Clear preceding floats on both sides.
    Both,
}

/// The positioning model selected for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PositionValue {
    /// Participate in normal flow and apply relative offsets.
    Relative,
    /// Position against a containing block.
    Absolute,
    /// Position against the surface viewport.
    Fixed,
    /// Switch between relative and fixed behavior while scrolling.
    Sticky,
}

/// Whether declared sizes include padding and border.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoxSizingValue {
    /// Sizes describe the content box.
    ContentBox,
    /// Sizes describe the border box.
    BorderBox,
}

/// Inline writing direction used to resolve logical edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DirectionValue {
    /// Inline start is the left edge.
    Ltr,
    /// Inline start is the right edge.
    Rtl,
}

/// A size constraint before containing-block percentage resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SizeValue {
    /// Let the layout algorithm choose.
    Auto,
    /// Explicit length, percentage, or affine `calc` expression.
    LengthPercentage(LengthPercentageValue),
    /// Maximum intrinsic content size.
    MaxContent,
    /// Minimum intrinsic content size.
    MinContent,
    /// Fit content, optionally capped by a limit.
    FitContent(Option<LengthPercentageValue>),
    /// No maximum constraint.
    None,
}

/// A margin or inset value that may be automatic.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LengthPercentageAutoValue {
    /// Automatic value.
    Auto,
    /// Explicit length or percentage.
    LengthPercentage(LengthPercentageValue),
}

/// Main-axis direction for flex layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlexDirectionValue {
    /// Inline direction.
    Row,
    /// Reversed inline direction.
    RowReverse,
    /// Block direction.
    Column,
    /// Reversed block direction.
    ColumnReverse,
}

/// Flex line wrapping behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlexWrapValue {
    /// Keep a single line.
    NoWrap,
    /// Wrap in the cross-axis direction.
    Wrap,
    /// Wrap in the reverse cross-axis direction.
    WrapReverse,
}

/// Flex basis before containing-block percentage resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FlexBasisValue {
    /// Use the item's main size.
    Auto,
    /// Use intrinsic content size.
    Content,
    /// Explicit basis.
    LengthPercentage(LengthPercentageValue),
}

/// Minimum sizing function for one specified CSS Grid track.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridMinTrackSizingValue {
    /// A fixed length or percentage.
    Fixed(LengthPercentageValue),
    /// The min-content contribution.
    MinContent,
    /// The max-content contribution.
    MaxContent,
    /// Automatic minimum sizing.
    Auto,
}

/// Maximum sizing function for one specified CSS Grid track.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridMaxTrackSizingValue {
    /// A fixed length or percentage.
    Fixed(LengthPercentageValue),
    /// The min-content contribution.
    MinContent,
    /// The max-content contribution.
    MaxContent,
    /// Fit content up to a limit.
    FitContent(LengthPercentageValue),
    /// Automatic maximum sizing.
    Auto,
    /// A flexible `fr` share.
    Fraction(StyleNumber),
}

/// Specified minimum and maximum sizing functions for one Grid track.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTrackSizingValue {
    /// Minimum sizing function.
    pub min: GridMinTrackSizingValue,
    /// Maximum sizing function.
    pub max: GridMaxTrackSizingValue,
}

/// One repeated fragment in a specified Grid template.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GridTemplateRepetitionValue {
    /// Repetition count.
    pub count: crate::GridRepetitionCountValue,
    /// Tracks inside the repeated fragment.
    pub tracks: Vec<GridTrackSizingValue>,
    /// Named lines surrounding the repeated tracks.
    pub line_names: Vec<Vec<String>>,
}

/// One component in a specified Grid template.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GridTemplateComponentValue {
    /// One non-repeated track.
    Track(GridTrackSizingValue),
    /// A `repeat()` fragment.
    Repeat(GridTemplateRepetitionValue),
}

/// Specified track components and named lines for one Grid axis.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GridTemplateValue {
    /// Track or repetition components.
    pub components: Vec<GridTemplateComponentValue>,
    /// Named lines outside repetition components.
    pub line_names: Vec<Vec<String>>,
}

/// Main-axis distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JustifyContentValue {
    /// Stretch eligible items.
    Stretch,
    /// Pack at flex start.
    FlexStart,
    /// Pack at flex end.
    FlexEnd,
    /// Center items.
    Center,
    /// Distribute space between items.
    SpaceBetween,
    /// Distribute space around items.
    SpaceAround,
    /// Distribute equal space between and around items.
    SpaceEvenly,
    /// Pack at logical start.
    Start,
    /// Pack at logical end.
    End,
}

/// Cross-axis item alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignItemsValue {
    /// Stretch items.
    Stretch,
    /// Align at flex start.
    FlexStart,
    /// Align at flex end.
    FlexEnd,
    /// Center items.
    Center,
    /// Align text baselines.
    Baseline,
    /// Align at logical start.
    Start,
    /// Align at logical end.
    End,
}

/// Per-item cross-axis alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignSelfValue {
    /// Defer to the parent's `align-items`.
    Auto,
    /// Stretch this item.
    Stretch,
    /// Align at flex start.
    FlexStart,
    /// Align at flex end.
    FlexEnd,
    /// Center this item.
    Center,
    /// Align its baseline.
    Baseline,
    /// Align at logical start.
    Start,
    /// Align at logical end.
    End,
}

/// Cross-axis line distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignContentValue {
    /// Stretch lines.
    Stretch,
    /// Pack lines at flex start.
    FlexStart,
    /// Pack lines at flex end.
    FlexEnd,
    /// Center lines.
    Center,
    /// Distribute space between lines.
    SpaceBetween,
    /// Distribute space around lines.
    SpaceAround,
    /// Distribute equal space between and around lines.
    SpaceEvenly,
}

/// A specified width-to-height ratio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AspectRatioValue {
    width: StyleNumber,
    height: StyleNumber,
}

impl AspectRatioValue {
    /// Stores the width and height components for resolver validation.
    pub const fn new(width: f32, height: f32) -> Self {
        Self {
            width: StyleNumber::new(width),
            height: StyleNumber::new(height),
        }
    }

    /// Returns the specified width component.
    pub const fn width(self) -> f32 {
        self.width.get()
    }

    /// Returns the specified height component.
    pub const fn height(self) -> f32 {
        self.height.get()
    }
}
