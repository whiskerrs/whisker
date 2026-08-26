//! Renderer-independent values stored in specified style declarations.

/// An `f32` with equality and hashing defined by its IEEE-754 bit pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleNumber(u32);

impl StyleNumber {
    /// Stores a number without changing its representation.
    pub const fn new(value: f32) -> Self {
        Self(value.to_bits())
    }

    /// Returns the stored number.
    pub const fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// A unit accepted by Whisker's length model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    /// Logical pixels: iOS points and Android density-independent pixels.
    Px,
    /// Responsive pixels relative to a 750-unit viewport width.
    Rpx,
    /// Physical device pixels.
    Ppx,
    /// Units relative to the element's computed font size.
    Em,
    /// Units relative to the root computed font size.
    Rem,
    /// Percent of viewport height.
    Vh,
    /// Percent of viewport width.
    Vw,
}

/// A semantic length that does not require CSS parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthValue {
    /// Unitless zero.
    Zero,
    /// A number paired with an explicit unit.
    Dimension {
        /// Numeric magnitude.
        value: StyleNumber,
        /// Length unit.
        unit: LengthUnit,
    },
}

/// A semantic length-or-percentage value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LengthPercentageValue {
    /// Absolute or environment-relative length.
    Length(LengthValue),
    /// Percentage number before the `%` suffix.
    Percentage(StyleNumber),
    /// Typed arithmetic expression.
    Calc(Box<CalcExpression>),
}

/// A typed arithmetic expression used by length-percentage values.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CalcExpression {
    /// Length-percentage operand.
    Value(Box<LengthPercentageValue>),
    /// Unitless numeric operand.
    Number(StyleNumber),
    /// Addition.
    Add(Box<Self>, Box<Self>),
    /// Subtraction.
    Sub(Box<Self>, Box<Self>),
    /// Multiplication.
    Mul(Box<Self>, Box<Self>),
    /// Division.
    Div(Box<Self>, Box<Self>),
}

/// A font family selected by application code or by the platform default.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFamilyValue {
    /// The platform's default application font.
    System,
    /// One explicitly named font family.
    Named(String),
}

/// The face style used to render text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyleValue {
    /// An upright face.
    Normal,
    /// An italic face.
    Italic,
    /// An oblique or synthesized slanted face.
    Oblique,
}

/// A numeric font weight in the CSS-compatible `1..=1000` range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontWeightValue(u16);

impl FontWeightValue {
    /// The normal font weight.
    pub const NORMAL: Self = Self(400);

    /// The bold font weight.
    pub const BOLD: Self = Self(700);

    /// Creates a font weight when it is in the supported range.
    pub const fn new(value: u16) -> Option<Self> {
        if value >= 1 && value <= 1000 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Preserves an authoring value for later resolver validation.
    ///
    /// Prefer [`Self::new`] outside compatibility adapters.
    pub const fn from_raw(value: u16) -> Self {
        Self(value)
    }

    /// Returns the numeric weight.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A semantic color that does not require parsing CSS text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColorValue {
    /// A named color accepted by Whisker's typed authoring API.
    Named(String),
    /// An sRGB color with a floating-point alpha channel.
    Rgba {
        /// Red channel.
        red: u8,
        /// Green channel.
        green: u8,
        /// Blue channel.
        blue: u8,
        /// Alpha in the range `0.0..=1.0`.
        alpha: StyleNumber,
    },
    /// An HSL color, normalized to degrees and percentages.
    Hsla {
        /// Hue in degrees.
        hue_degrees: StyleNumber,
        /// Saturation percentage.
        saturation: StyleNumber,
        /// Lightness percentage.
        lightness: StyleNumber,
        /// Alpha in the range `0.0..=1.0`.
        alpha: StyleNumber,
    },
}

/// A specified line height before environment and font-size resolution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LineHeightValue {
    /// Let the platform text shaper select its normal line height.
    Normal,
    /// A multiplier of the node's computed font size.
    Number(StyleNumber),
    /// An explicit length or percentage of the node's computed font size.
    LengthPercentage(LengthPercentageValue),
}

/// A specified corner radius with independent horizontal and vertical axes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BorderRadiusValue {
    /// Radius resolved against the border box width.
    pub horizontal: LengthPercentageValue,
    /// Radius resolved against the border box height.
    pub vertical: LengthPercentageValue,
}

/// One specified background image that does not contain Host resource IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundImageValue {
    /// An explicit `none` layer retained for future list alignment.
    None,
    /// URL text resolved under Host resource policy.
    Url(String),
}

/// Tiling behavior on one background axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundRepeatModeValue {
    /// Repeat adjacent tiles without adding space.
    Repeat,
    /// Paint at most one tile.
    NoRepeat,
    /// Distribute whole tiles with equal gaps.
    Space,
    /// Resize tiles so a whole number fills the axis.
    Round,
}

/// Expanded two-axis value of one `background-repeat` layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundRepeatValue {
    /// Horizontal tiling behavior.
    pub horizontal: BackgroundRepeatModeValue,
    /// Vertical tiling behavior.
    pub vertical: BackgroundRepeatModeValue,
}

/// Specified position of one background layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundPositionValue {
    /// Horizontal position relative to the positioning area.
    pub horizontal: LengthPercentageValue,
    /// Vertical position relative to the positioning area.
    pub vertical: LengthPercentageValue,
}

/// Specified size of one background layer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundSizeValue {
    /// Use the image's intrinsic dimensions.
    Auto,
    /// Preserve aspect ratio while covering the positioning area.
    Cover,
    /// Preserve aspect ratio while fitting inside the positioning area.
    Contain,
    /// Resolve an explicit width and height.
    Explicit {
        /// Specified image width, or intrinsic width for `auto`.
        width: Option<LengthPercentageValue>,
        /// Specified image height, or intrinsic height for `auto`.
        height: Option<LengthPercentageValue>,
    },
}

/// Box used to position or clip a background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundBoxValue {
    /// The outer border box.
    Border,
    /// The inner padding box.
    Padding,
    /// The content box.
    Content,
}

/// Scrolling relationship of one background layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundAttachmentValue {
    /// Scroll with the element's border box.
    Scroll,
    /// Remain fixed relative to the viewport.
    Fixed,
    /// Scroll with the element's contents.
    Local,
}

/// An owned value in a specified inline-style declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StyleValue {
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Integer(i64),
    /// Unitless real number.
    Number(StyleNumber),
    /// UTF-8 text whose interpretation is defined by its property schema.
    Text(String),
    /// Length value.
    Length(LengthValue),
    /// Length, percentage, or typed `calc` expression.
    LengthPercentage(LengthPercentageValue),
    /// One border corner's horizontal and vertical radii.
    BorderRadius(BorderRadiusValue),
    /// Ordered background images, front to back.
    BackgroundImages(Vec<BackgroundImageValue>),
    /// Expanded tiling behavior of one background layer.
    BackgroundRepeat(BackgroundRepeatValue),
    /// Specified position of one background layer.
    BackgroundPosition(BackgroundPositionValue),
    /// Specified size of one background layer.
    BackgroundSize(BackgroundSizeValue),
    /// Positioning or clipping box of one background layer.
    BackgroundBox(BackgroundBoxValue),
    /// Scrolling relationship of one background layer.
    BackgroundAttachment(BackgroundAttachmentValue),
    /// Font family.
    FontFamily(FontFamilyValue),
    /// Font face style.
    FontStyle(FontStyleValue),
    /// Numeric font weight.
    FontWeight(FontWeightValue),
    /// Text color.
    Color(ColorValue),
    /// Border line style.
    BorderStyle(crate::BorderStyleValue),
    /// Overflow behavior on one axis.
    Overflow(crate::OverflowValue),
    /// Paint visibility.
    Visibility(crate::VisibilityValue),
    /// Line height.
    LineHeight(LineHeightValue),
    /// Layout algorithm.
    Display(crate::DisplayValue),
    /// Positioning model.
    Position(crate::PositionValue),
    /// Box sizing model.
    BoxSizing(crate::BoxSizingValue),
    /// Inline writing direction.
    Direction(crate::DirectionValue),
    /// Width or height constraint.
    Size(crate::SizeValue),
    /// Margin or inset value.
    LengthPercentageAuto(crate::LengthPercentageAutoValue),
    /// Flex main-axis direction.
    FlexDirection(crate::FlexDirectionValue),
    /// Flex wrapping behavior.
    FlexWrap(crate::FlexWrapValue),
    /// Flex basis.
    FlexBasis(crate::FlexBasisValue),
    /// Main-axis distribution.
    JustifyContent(crate::JustifyContentValue),
    /// Cross-axis item alignment.
    AlignItems(crate::AlignItemsValue),
    /// Per-item cross-axis alignment.
    AlignSelf(crate::AlignSelfValue),
    /// Cross-axis line distribution.
    AlignContent(crate::AlignContentValue),
    /// Explicit Grid track template for one axis.
    GridTemplate(crate::GridTemplateValue),
    /// Implicit Grid track sizing functions for one axis.
    GridTracks(Vec<crate::GridTrackSizingValue>),
    /// Grid auto-placement direction and density.
    GridAutoFlow(crate::GridAutoFlowValue),
    /// One Grid item edge placement.
    GridPlacement(crate::GridPlacementValue),
    /// Named Grid area rectangles.
    GridTemplateAreas(crate::GridTemplateAreasValue),
    /// Width-to-height ratio.
    AspectRatio(crate::AspectRatioValue),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn style_number_preserves_bits_and_hashes() {
        let number = StyleNumber::new(-0.0);
        assert_eq!(number.get().to_bits(), (-0.0_f32).to_bits());
        let mut values = HashSet::new();
        assert!(values.insert(number));
        assert!(!values.insert(number));
    }

    #[test]
    fn semantic_values_clone_and_compare_without_text() {
        let length = LengthValue::Dimension {
            value: StyleNumber::new(12.5),
            unit: LengthUnit::Px,
        };
        let calc = CalcExpression::Add(
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Length(length),
            ))),
            Box::new(CalcExpression::Value(Box::new(
                LengthPercentageValue::Percentage(StyleNumber::new(50.0)),
            ))),
        );
        let value = StyleValue::LengthPercentage(LengthPercentageValue::Calc(Box::new(calc)));
        assert_eq!(value.clone(), value);
        assert_ne!(value, StyleValue::Text("calc(12.5px + 50%)".into()));
    }

    #[test]
    fn scalar_variants_remain_distinct() {
        assert_ne!(StyleValue::Bool(true), StyleValue::Integer(1));
        assert_ne!(
            StyleValue::Number(StyleNumber::new(1.0)),
            StyleValue::Text("1".into())
        );
        assert_eq!(
            StyleValue::Length(LengthValue::Zero),
            StyleValue::Length(LengthValue::Zero)
        );
    }

    #[test]
    fn font_weight_accepts_only_the_supported_range() {
        assert_eq!(FontWeightValue::new(1).unwrap().get(), 1);
        assert_eq!(FontWeightValue::new(1000).unwrap().get(), 1000);
        assert_eq!(FontWeightValue::new(0), None);
        assert_eq!(FontWeightValue::new(1001), None);
        assert_eq!(FontWeightValue::NORMAL.get(), 400);
        assert_eq!(FontWeightValue::BOLD.get(), 700);
    }

    #[test]
    fn typography_variants_are_semantically_distinct() {
        assert_ne!(
            StyleValue::FontFamily(FontFamilyValue::System),
            StyleValue::Text("system-ui".into())
        );
        assert_ne!(
            StyleValue::FontStyle(FontStyleValue::Italic),
            StyleValue::FontWeight(FontWeightValue::BOLD)
        );
        assert_ne!(
            StyleValue::Color(ColorValue::Named("red".into())),
            StyleValue::LineHeight(LineHeightValue::Normal)
        );
    }
}
