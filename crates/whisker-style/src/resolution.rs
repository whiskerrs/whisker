//! Deterministic resolution for Whisker's fixed inherited text context.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CalcExpression, ColorValue, ComputedLayoutStyle, ComputedMotionStyle, ComputedPaintStyle,
    CursorValue, CustomPropertyName, CustomPropertyReference, DirectionValue, FontFamilyValue,
    FontFeatureValue, FontOpticalSizingValue, FontStyleValue, FontVariationValue, FontWeightValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, PointerEventsValue,
    SpecifiedStyle, StyleNumber, StyleProperty, StyleValue, TextAlignValue,
    TextDecorationLineValue, TextDecorationStyleValue, TextDecorationValue, TextOverflowValue,
    TextShadowValue, WhiteSpaceValue, WordBreakValue,
};

const RPX_REFERENCE_WIDTH: f32 = 750.0;

/// Environment values needed to resolve relative style units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleEnvironment {
    viewport_width: StyleNumber,
    viewport_height: StyleNumber,
    scale_factor: StyleNumber,
    root_font_size: StyleNumber,
}

impl StyleEnvironment {
    /// Creates an environment using logical-pixel viewport dimensions.
    pub const fn new(
        viewport_width: f32,
        viewport_height: f32,
        scale_factor: f32,
        root_font_size: f32,
    ) -> Self {
        Self {
            viewport_width: StyleNumber::new(viewport_width),
            viewport_height: StyleNumber::new(viewport_height),
            scale_factor: StyleNumber::new(scale_factor),
            root_font_size: StyleNumber::new(root_font_size),
        }
    }

    /// Returns the logical viewport width.
    pub const fn viewport_width(self) -> f32 {
        self.viewport_width.get()
    }

    /// Returns the logical viewport height.
    pub const fn viewport_height(self) -> f32 {
        self.viewport_height.get()
    }

    /// Returns physical pixels per logical pixel.
    pub const fn scale_factor(self) -> f32 {
        self.scale_factor.get()
    }

    /// Returns the root font size in logical pixels.
    pub const fn root_font_size(self) -> f32 {
        self.root_font_size.get()
    }

    fn validate(self) -> Result<(), StyleResolutionError> {
        if !self.viewport_width().is_finite()
            || self.viewport_width() < 0.0
            || !self.viewport_height().is_finite()
            || self.viewport_height() < 0.0
            || !self.scale_factor().is_finite()
            || self.scale_factor() <= 0.0
            || !self.root_font_size().is_finite()
            || self.root_font_size() < 0.0
        {
            return Err(StyleResolutionError::InvalidEnvironment);
        }
        Ok(())
    }
}

impl Default for StyleEnvironment {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0, 14.0)
    }
}

/// A computed line height ready for text measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedLineHeight {
    /// Let the platform text shaper use its normal metric.
    Normal,
    /// An explicit logical-pixel line height.
    LogicalPixels(StyleNumber),
}

/// Computed Lynx `text-indent` value.
///
/// Percentages remain unresolved until the text element has a definite width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComputedTextIndent {
    /// Fixed logical pixels after resolving environment-relative units.
    LogicalPixels(StyleNumber),
    /// Percentage number before the `%` suffix.
    Percentage(StyleNumber),
}

impl Default for ComputedTextIndent {
    fn default() -> Self {
        Self::LogicalPixels(StyleNumber::new(0.0))
    }
}

/// One resolved inherited text shadow.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedTextShadow {
    offset_x: StyleNumber,
    offset_y: StyleNumber,
    blur_radius: StyleNumber,
    color: ColorValue,
}

impl ComputedTextShadow {
    /// Returns the horizontal offset in logical pixels.
    pub const fn offset_x(&self) -> f32 {
        self.offset_x.get()
    }
    /// Returns the vertical offset in logical pixels.
    pub const fn offset_y(&self) -> f32 {
        self.offset_y.get()
    }
    /// Returns the blur radius in logical pixels.
    pub const fn blur_radius(&self) -> f32 {
        self.blur_radius.get()
    }
    /// Returns the shadow color.
    pub const fn color(&self) -> &ColorValue {
        &self.color
    }
}

/// One resolved inherited Lynx text decoration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedTextDecoration {
    line: TextDecorationLineValue,
    style: TextDecorationStyleValue,
    color: ColorValue,
}

impl ComputedTextDecoration {
    /// Returns the selected line kind.
    pub const fn line(&self) -> TextDecorationLineValue {
        self.line
    }
    /// Returns the selected stroke style.
    pub const fn style(&self) -> TextDecorationStyleValue {
        self.style
    }
    /// Returns the resolved decoration color.
    pub const fn color(&self) -> &ColorValue {
        &self.color
    }
}

/// The computed text values inherited by descendant nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InheritedStyle {
    custom_properties: BTreeMap<CustomPropertyName, StyleValue>,
    cursor: CursorValue,
    pointer_events: PointerEventsValue,
    direction: DirectionValue,
    font_family: FontFamilyValue,
    font_features: Vec<FontFeatureValue>,
    font_variations: Vec<FontVariationValue>,
    font_optical_sizing: FontOpticalSizingValue,
    font_size: StyleNumber,
    font_weight: FontWeightValue,
    font_style: FontStyleValue,
    line_height: ComputedLineHeight,
    letter_spacing: StyleNumber,
    color: ColorValue,
    text_align: TextAlignValue,
    text_decoration: ComputedTextDecoration,
    text_shadow: Option<ComputedTextShadow>,
}

impl InheritedStyle {
    fn initial(environment: StyleEnvironment) -> Self {
        Self {
            custom_properties: BTreeMap::new(),
            cursor: CursorValue::Auto,
            pointer_events: PointerEventsValue::Auto,
            direction: DirectionValue::Ltr,
            font_family: FontFamilyValue::System,
            font_features: Vec::new(),
            font_variations: Vec::new(),
            font_optical_sizing: FontOpticalSizingValue::None,
            font_size: StyleNumber::new(environment.root_font_size()),
            font_weight: FontWeightValue::NORMAL,
            font_style: FontStyleValue::Normal,
            line_height: ComputedLineHeight::Normal,
            letter_spacing: StyleNumber::new(0.0),
            color: ColorValue::Rgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: StyleNumber::new(1.0),
            },
            text_align: TextAlignValue::Start,
            text_decoration: ComputedTextDecoration {
                line: TextDecorationLineValue::None,
                style: TextDecorationStyleValue::Solid,
                color: ColorValue::Rgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: StyleNumber::new(1.0),
                },
            },
            text_shadow: None,
        }
    }

    /// Returns the inherited pointing-device cursor.
    pub const fn cursor(&self) -> CursorValue {
        self.cursor
    }

    /// Returns the inherited pointer hit-test participation.
    pub const fn pointer_events(&self) -> PointerEventsValue {
        self.pointer_events
    }

    /// Returns the inherited inline writing direction.
    pub const fn direction(&self) -> DirectionValue {
        self.direction
    }

    /// Returns one resolved inherited custom property.
    pub fn custom_property(&self, name: &CustomPropertyName) -> Option<&StyleValue> {
        self.custom_properties.get(name)
    }

    /// Iterates over resolved inherited custom properties in name order.
    pub fn custom_properties(&self) -> impl Iterator<Item = (&CustomPropertyName, &StyleValue)> {
        self.custom_properties.iter()
    }

    /// Returns the selected font family.
    pub const fn font_family(&self) -> &FontFamilyValue {
        &self.font_family
    }

    /// Returns sorted, unique OpenType feature settings.
    pub fn font_features(&self) -> &[FontFeatureValue] {
        &self.font_features
    }

    /// Returns sorted, unique variable-font axis settings.
    pub fn font_variations(&self) -> &[FontVariationValue] {
        &self.font_variations
    }

    /// Returns inherited Lynx optical sizing behavior.
    pub const fn font_optical_sizing(&self) -> FontOpticalSizingValue {
        self.font_optical_sizing
    }

    /// Returns the computed logical-pixel font size.
    pub const fn font_size(&self) -> f32 {
        self.font_size.get()
    }

    /// Returns the numeric font weight.
    pub const fn font_weight(&self) -> FontWeightValue {
        self.font_weight
    }

    /// Returns the selected font face style.
    pub const fn font_style(&self) -> FontStyleValue {
        self.font_style
    }

    /// Returns the computed line height.
    pub const fn line_height(&self) -> ComputedLineHeight {
        self.line_height
    }

    /// Returns computed logical-pixel letter spacing.
    pub const fn letter_spacing(&self) -> f32 {
        self.letter_spacing.get()
    }

    /// Returns the text color.
    pub const fn color(&self) -> &ColorValue {
        &self.color
    }

    /// Returns inline text alignment.
    pub const fn text_align(&self) -> TextAlignValue {
        self.text_align
    }

    /// Returns the inherited single-line text decoration.
    pub const fn text_decoration(&self) -> &ComputedTextDecoration {
        &self.text_decoration
    }

    /// Returns the optional single inherited text shadow.
    pub const fn text_shadow(&self) -> Option<&ComputedTextShadow> {
        self.text_shadow.as_ref()
    }

    /// Classifies inherited changes and their downstream work.
    pub fn changes_from(&self, previous: &Self) -> InheritedStyleChange {
        let mut properties = InheritedPropertySet::EMPTY;
        let mut impacts = PropertyImpactSet::EMPTY;
        if self.cursor != previous.cursor {
            properties |= InheritedPropertySet::CURSOR;
            impacts |= PropertyImpactSet::INPUT;
        }
        if self.pointer_events != previous.pointer_events {
            properties |= InheritedPropertySet::POINTER_EVENTS;
            impacts |= PropertyImpactSet::INPUT;
        }
        if self.direction != previous.direction {
            properties |= InheritedPropertySet::DIRECTION;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.custom_properties != previous.custom_properties {
            properties |= InheritedPropertySet::CUSTOM_PROPERTIES;
            impacts |= PropertyImpactSet::ALL;
        }
        if self.font_family != previous.font_family {
            properties |= InheritedPropertySet::FONT_FAMILY;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_features != previous.font_features {
            properties |= InheritedPropertySet::FONT_FEATURE_SETTINGS;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_variations != previous.font_variations {
            properties |= InheritedPropertySet::FONT_VARIATION_SETTINGS;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_optical_sizing != previous.font_optical_sizing {
            properties |= InheritedPropertySet::FONT_OPTICAL_SIZING;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_size != previous.font_size {
            properties |= InheritedPropertySet::FONT_SIZE;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_weight != previous.font_weight {
            properties |= InheritedPropertySet::FONT_WEIGHT;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.font_style != previous.font_style {
            properties |= InheritedPropertySet::FONT_STYLE;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.line_height != previous.line_height {
            properties |= InheritedPropertySet::LINE_HEIGHT;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.letter_spacing != previous.letter_spacing {
            properties |= InheritedPropertySet::LETTER_SPACING;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.color != previous.color {
            properties |= InheritedPropertySet::COLOR;
            impacts |= PropertyImpactSet::PAINT;
        }
        if self.text_align != previous.text_align {
            properties |= InheritedPropertySet::TEXT_ALIGN;
            impacts |= PropertyImpactSet::TEXT_METRICS;
        }
        if self.text_decoration != previous.text_decoration {
            properties |= InheritedPropertySet::TEXT_DECORATION;
            impacts |= PropertyImpactSet::PAINT;
        }
        if self.text_shadow != previous.text_shadow {
            properties |= InheritedPropertySet::TEXT_SHADOW;
            impacts |= PropertyImpactSet::PAINT;
        }
        InheritedStyleChange {
            properties,
            impacts,
        }
    }
}

/// The currently implemented computed-style slice.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    inherited_text: InheritedStyle,
    text_indent: ComputedTextIndent,
    white_space: WhiteSpaceValue,
    word_break: WordBreakValue,
    text_overflow: TextOverflowValue,
    layout: ComputedLayoutStyle,
    paint: ComputedPaintStyle,
    motion: ComputedMotionStyle,
}

impl ComputedStyle {
    /// Returns the resolved pointing-device cursor.
    pub const fn cursor(&self) -> CursorValue {
        self.inherited_text.cursor()
    }

    /// Returns whether this node participates in pointer hit testing.
    pub const fn pointer_events(&self) -> PointerEventsValue {
        self.inherited_text.pointer_events()
    }

    /// Returns computed text values for this node.
    pub const fn inherited_text(&self) -> &InheritedStyle {
        &self.inherited_text
    }

    /// Returns this node's non-inherited first-line indentation.
    pub const fn text_indent(&self) -> ComputedTextIndent {
        self.text_indent
    }

    /// Returns this node's non-inherited whitespace and wrapping policy.
    pub const fn white_space(&self) -> WhiteSpaceValue {
        self.white_space
    }

    /// Returns this node's non-inherited word-breaking policy.
    pub const fn word_break(&self) -> WordBreakValue {
        self.word_break
    }

    /// Returns this node's non-inherited text overflow treatment.
    pub const fn text_overflow(&self) -> TextOverflowValue {
        self.text_overflow
    }

    /// Returns Taffy-independent computed layout input for this node.
    pub const fn layout(&self) -> &ComputedLayoutStyle {
        &self.layout
    }

    /// Returns Host-independent computed paint input for this node.
    pub const fn paint(&self) -> &ComputedPaintStyle {
        &self.paint
    }

    /// Returns Host-independent transition and keyframe timeline settings.
    pub const fn motion(&self) -> &ComputedMotionStyle {
        &self.motion
    }
}

/// Result of resolving one node's specified style.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResolvedNodeStyle {
    computed: ComputedStyle,
}

impl ResolvedNodeStyle {
    /// Returns the node's computed style.
    pub const fn computed(&self) -> &ComputedStyle {
        &self.computed
    }

    /// Returns the fixed text context to pass to every child.
    pub const fn inherited_for_children(&self) -> &InheritedStyle {
        self.computed.inherited_text()
    }
}

/// Failure to turn a typed declaration into a valid computed value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleResolutionError {
    /// One or more environment inputs are non-finite or outside their range.
    InvalidEnvironment,
    /// A property received the wrong semantic variant or an invalid number.
    InvalidPropertyValue(StyleProperty),
    /// A typed `calc` expression has incompatible dimensions or divides by zero.
    InvalidCalculation(StyleProperty),
}

impl fmt::Display for StyleResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEnvironment => formatter.write_str("invalid style environment"),
            Self::InvalidPropertyValue(property) => {
                write!(formatter, "invalid value for `{}`", property.css_name())
            }
            Self::InvalidCalculation(property) => {
                write!(
                    formatter,
                    "invalid calculation for `{}`",
                    property.css_name()
                )
            }
        }
    }
}

impl std::error::Error for StyleResolutionError {}

/// Bit set identifying which inherited values changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InheritedPropertySet(u32);

impl InheritedPropertySet {
    /// No inherited property.
    pub const EMPTY: Self = Self(0);
    /// `font-family`.
    pub const FONT_FAMILY: Self = Self(1 << 0);
    /// `font-size`.
    pub const FONT_SIZE: Self = Self(1 << 1);
    /// `font-weight`.
    pub const FONT_WEIGHT: Self = Self(1 << 2);
    /// `font-style`.
    pub const FONT_STYLE: Self = Self(1 << 3);
    /// `line-height`.
    pub const LINE_HEIGHT: Self = Self(1 << 4);
    /// `letter-spacing`.
    pub const LETTER_SPACING: Self = Self(1 << 5);
    /// `color`.
    pub const COLOR: Self = Self(1 << 6);
    /// `text-shadow`.
    pub const TEXT_SHADOW: Self = Self(1 << 7);
    /// `text-decoration`.
    pub const TEXT_DECORATION: Self = Self(1 << 8);
    /// `text-align`.
    pub const TEXT_ALIGN: Self = Self(1 << 9);
    /// `font-feature-settings`.
    pub const FONT_FEATURE_SETTINGS: Self = Self(1 << 10);
    /// `font-variation-settings`.
    pub const FONT_VARIATION_SETTINGS: Self = Self(1 << 11);
    /// `font-optical-sizing`.
    pub const FONT_OPTICAL_SIZING: Self = Self(1 << 12);
    /// `cursor`.
    pub const CURSOR: Self = Self(1 << 13);
    /// `pointer-events`.
    pub const POINTER_EVENTS: Self = Self(1 << 14);
    /// Inherited CSS custom-property environment.
    pub const CUSTOM_PROPERTIES: Self = Self(1 << 15);
    /// `direction`.
    pub const DIRECTION: Self = Self(1 << 16);

    /// Returns whether this set contains every bit from `other`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether no properties are present.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOrAssign for InheritedPropertySet {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Work categories invalidated by a computed style change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PropertyImpactSet(u8);

impl PropertyImpactSet {
    /// No downstream work.
    pub const EMPTY: Self = Self(0);
    /// Intrinsic measurement must be refreshed.
    pub const INTRINSIC_MEASURE: Self = Self(1 << 0);
    /// Box layout must run again.
    pub const LAYOUT: Self = Self(1 << 1);
    /// Paint output changed.
    pub const PAINT: Self = Self(1 << 2);
    /// Hit testing or pointer presentation changed.
    pub const INPUT: Self = Self(1 << 3);
    /// All work caused by a text metric change.
    pub const TEXT_METRICS: Self = Self(Self::INTRINSIC_MEASURE.0 | Self::LAYOUT.0 | Self::PAINT.0);
    /// Every downstream style-dependent work category.
    pub const ALL: Self =
        Self(Self::INTRINSIC_MEASURE.0 | Self::LAYOUT.0 | Self::PAINT.0 | Self::INPUT.0);

    /// Returns whether this set contains every bit from `other`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether no impacts are present.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOrAssign for PropertyImpactSet {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Inherited-property delta used to invalidate descendant style contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InheritedStyleChange {
    properties: InheritedPropertySet,
    impacts: PropertyImpactSet,
}

impl InheritedStyleChange {
    /// Returns the inherited values that changed.
    pub const fn properties(self) -> InheritedPropertySet {
        self.properties
    }

    /// Returns downstream work required by the change.
    pub const fn impacts(self) -> PropertyImpactSet {
        self.impacts
    }

    /// Returns whether this delta contains no changes.
    pub const fn is_empty(self) -> bool {
        self.properties.is_empty()
    }
}

/// Resolves all currently implemented computed-style slices for one node.
pub fn resolve_style(
    specified: &SpecifiedStyle,
    parent: Option<&InheritedStyle>,
    environment: StyleEnvironment,
) -> Result<ResolvedNodeStyle, StyleResolutionError> {
    environment.validate()?;
    let initial = InheritedStyle::initial(environment);
    let base = parent.unwrap_or(&initial);
    let (specified, custom_properties) = materialize_custom_properties(specified, base);
    let specified = &specified;
    let declarations = InheritedDeclarations::from_specified(specified);

    let cursor = match declarations.cursor {
        Some(StyleValue::Cursor(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::Cursor)),
        None => base.cursor,
    };
    let pointer_events = match declarations.pointer_events {
        Some(StyleValue::PointerEvents(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::PointerEvents)),
        None => base.pointer_events,
    };

    let font_size = match declarations.font_size {
        Some(value) => {
            let value = expect_length_percentage(StyleProperty::FontSize, value)?;
            let pixels = resolve_length_percentage(
                value,
                base.font_size(),
                base.font_size(),
                environment,
                StyleProperty::FontSize,
            )?;
            if pixels < 0.0 {
                return Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::FontSize,
                ));
            }
            StyleNumber::new(pixels)
        }
        None => base.font_size,
    };

    let font_family = match declarations.font_family {
        Some(StyleValue::FontFamily(FontFamilyValue::Named(name))) if name.is_empty() => {
            return Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::FontFamily,
            ));
        }
        Some(StyleValue::FontFamily(value)) => value.clone(),
        Some(_) => return Err(wrong_type(StyleProperty::FontFamily)),
        None => base.font_family.clone(),
    };
    let font_features = match declarations.font_features {
        Some(StyleValue::FontFeatures(values)) => canonical_features(values),
        Some(_) => return Err(wrong_type(StyleProperty::FontFeatureSettings)),
        None => base.font_features.clone(),
    };
    let font_variations = match declarations.font_variations {
        Some(StyleValue::FontVariations(values)) => canonical_variations(values)?,
        Some(_) => return Err(wrong_type(StyleProperty::FontVariationSettings)),
        None => base.font_variations.clone(),
    };
    let font_optical_sizing = match declarations.font_optical_sizing {
        Some(StyleValue::FontOpticalSizing(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::FontOpticalSizing)),
        None => base.font_optical_sizing,
    };
    let font_weight = match declarations.font_weight {
        Some(StyleValue::FontWeight(value)) if FontWeightValue::new(value.get()).is_some() => {
            *value
        }
        Some(StyleValue::FontWeight(_)) => {
            return Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::FontWeight,
            ));
        }
        Some(_) => return Err(wrong_type(StyleProperty::FontWeight)),
        None => base.font_weight,
    };
    let font_style = match declarations.font_style {
        Some(StyleValue::FontStyle(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::FontStyle)),
        None => base.font_style,
    };
    let line_height = match declarations.line_height {
        Some(StyleValue::LineHeight(LineHeightValue::Normal)) => ComputedLineHeight::Normal,
        Some(StyleValue::LineHeight(LineHeightValue::Number(value))) => {
            let multiplier = finite(*value, StyleProperty::LineHeight)?;
            let pixels = multiplier * font_size.get();
            if multiplier < 0.0 || !pixels.is_finite() {
                return Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::LineHeight,
                ));
            }
            ComputedLineHeight::LogicalPixels(StyleNumber::new(pixels))
        }
        Some(StyleValue::LineHeight(LineHeightValue::LengthPercentage(value))) => {
            let pixels = resolve_length_percentage(
                value,
                font_size.get(),
                font_size.get(),
                environment,
                StyleProperty::LineHeight,
            )?;
            if pixels < 0.0 {
                return Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::LineHeight,
                ));
            }
            ComputedLineHeight::LogicalPixels(StyleNumber::new(pixels))
        }
        Some(_) => return Err(wrong_type(StyleProperty::LineHeight)),
        None => base.line_height,
    };
    let letter_spacing = match declarations.letter_spacing {
        Some(StyleValue::Length(value)) => StyleNumber::new(resolve_length(
            *value,
            font_size.get(),
            environment,
            StyleProperty::LetterSpacing,
        )?),
        Some(_) => return Err(wrong_type(StyleProperty::LetterSpacing)),
        None => base.letter_spacing,
    };
    let color = match declarations.color {
        Some(StyleValue::Color(value)) => normalize_color(value)?,
        Some(_) => return Err(wrong_type(StyleProperty::Color)),
        None => base.color.clone(),
    };
    let text_align = match declarations.text_align {
        Some(StyleValue::TextAlign(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::TextAlign)),
        None => base.text_align,
    };
    let text_decoration = match declarations.text_decoration {
        Some(StyleValue::TextDecoration(TextDecorationValue {
            line,
            style,
            color: decoration_color,
        })) => ComputedTextDecoration {
            line: *line,
            style: *style,
            color: decoration_color
                .as_ref()
                .map(normalize_color)
                .transpose()?
                .unwrap_or_else(|| color.clone()),
        },
        Some(_) => return Err(wrong_type(StyleProperty::TextDecoration)),
        None => base.text_decoration.clone(),
    };
    let text_shadow = match declarations.text_shadow {
        Some(StyleValue::TextShadow(TextShadowValue::None)) => None,
        Some(StyleValue::TextShadow(TextShadowValue::Shadow {
            offset_x,
            offset_y,
            blur_radius,
            color,
        })) => {
            let offset_x = resolve_length(
                *offset_x,
                font_size.get(),
                environment,
                StyleProperty::TextShadow,
            )?;
            let offset_y = resolve_length(
                *offset_y,
                font_size.get(),
                environment,
                StyleProperty::TextShadow,
            )?;
            let blur_radius = resolve_length(
                *blur_radius,
                font_size.get(),
                environment,
                StyleProperty::TextShadow,
            )?;
            if blur_radius < 0.0 {
                return Err(StyleResolutionError::InvalidPropertyValue(
                    StyleProperty::TextShadow,
                ));
            }
            Some(ComputedTextShadow {
                offset_x: StyleNumber::new(offset_x),
                offset_y: StyleNumber::new(offset_y),
                blur_radius: StyleNumber::new(blur_radius),
                color: normalize_color(color)?,
            })
        }
        Some(_) => return Err(wrong_type(StyleProperty::TextShadow)),
        None => base.text_shadow.clone(),
    };
    let text_indent = match specified
        .resolved()
        .iter()
        .rev()
        .find(|declaration| declaration.property() == StyleProperty::TextIndent)
        .map(|declaration| declaration.value())
    {
        Some(StyleValue::LengthPercentage(LengthPercentageValue::Length(value))) => {
            ComputedTextIndent::LogicalPixels(StyleNumber::new(resolve_length(
                *value,
                font_size.get(),
                environment,
                StyleProperty::TextIndent,
            )?))
        }
        Some(StyleValue::LengthPercentage(LengthPercentageValue::Percentage(value))) => {
            ComputedTextIndent::Percentage(StyleNumber::new(finite(
                *value,
                StyleProperty::TextIndent,
            )?))
        }
        Some(StyleValue::LengthPercentage(LengthPercentageValue::Calc(_))) | Some(_) => {
            return Err(wrong_type(StyleProperty::TextIndent));
        }
        None => ComputedTextIndent::default(),
    };
    let local_text_value = |property| {
        specified
            .resolved()
            .iter()
            .rev()
            .find(|declaration| declaration.property() == property)
            .map(|declaration| declaration.value())
    };
    let white_space = match local_text_value(StyleProperty::WhiteSpace) {
        Some(StyleValue::WhiteSpace(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::WhiteSpace)),
        None => WhiteSpaceValue::default(),
    };
    let word_break = match local_text_value(StyleProperty::WordBreak) {
        Some(StyleValue::WordBreak(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::WordBreak)),
        None => WordBreakValue::default(),
    };
    let text_overflow = match local_text_value(StyleProperty::TextOverflow) {
        Some(StyleValue::TextOverflow(value)) => *value,
        Some(_) => return Err(wrong_type(StyleProperty::TextOverflow)),
        None => TextOverflowValue::default(),
    };

    let layout = crate::layout::resolve_layout_style(
        specified,
        font_size.get(),
        base.direction,
        environment,
    )?;
    let inherited_text = InheritedStyle {
        custom_properties,
        cursor,
        pointer_events,
        direction: layout.direction,
        font_family,
        font_features,
        font_variations,
        font_optical_sizing,
        font_size,
        font_weight,
        font_style,
        line_height,
        letter_spacing,
        color,
        text_align,
        text_decoration,
        text_shadow,
    };
    let paint = crate::paint::resolve_paint_style(
        specified,
        &inherited_text,
        layout.direction,
        environment,
    )?;
    let motion = crate::motion::resolve_motion_style(specified)?;

    Ok(ResolvedNodeStyle {
        computed: ComputedStyle {
            inherited_text,
            text_indent,
            white_space,
            word_break,
            text_overflow,
            layout,
            paint,
            motion,
        },
    })
}

fn materialize_custom_properties(
    specified: &SpecifiedStyle,
    inherited: &InheritedStyle,
) -> (SpecifiedStyle, BTreeMap<CustomPropertyName, StyleValue>) {
    let mut candidates = inherited.custom_properties.clone();
    let mut local_names = BTreeSet::new();
    for declaration in specified.resolved_custom() {
        local_names.insert(declaration.name().clone());
        candidates.insert(declaration.name().clone(), declaration.value().clone());
    }
    let cyclic = local_names
        .iter()
        .filter(|name| custom_property_reaches(name, name, &candidates, &mut BTreeSet::new()))
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut cache: BTreeMap<CustomPropertyName, Option<StyleValue>> = BTreeMap::new();
    let names = candidates.keys().cloned().collect::<Vec<_>>();
    for name in names {
        let mut visiting = Vec::new();
        let _ = resolve_custom_name(&name, &candidates, &cyclic, &mut cache, &mut visiting);
    }
    let computed = cache
        .into_iter()
        .filter_map(|(name, value)| value.map(|value| (name, value)))
        .collect::<BTreeMap<_, _>>();

    let mut materialized = SpecifiedStyle::new();
    for declaration in specified.declarations() {
        let value = resolve_value_from_computed(declaration.value(), &computed);
        if let Some(value) = value {
            materialized = materialized.push(declaration.property(), value);
        }
    }
    (materialized, computed)
}

fn custom_property_reaches(
    start: &CustomPropertyName,
    current: &CustomPropertyName,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    visited: &mut BTreeSet<CustomPropertyName>,
) -> bool {
    if !visited.insert(current.clone()) {
        return false;
    }
    let Some(value) = candidates.get(current) else {
        return false;
    };
    let mut references = Vec::new();
    collect_custom_references(value, &mut references);
    references
        .into_iter()
        .any(|next| next == start || custom_property_reaches(start, next, candidates, visited))
}

fn collect_custom_references<'a>(
    value: &'a StyleValue,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    if let StyleValue::Variable(reference) = value {
        collect_reference(reference, references);
        return;
    }
    crate::value_tree::visit_length_percentages(value, &mut |value| {
        collect_length_percentage_references(value, references);
    });
}

fn collect_reference<'a>(
    reference: &'a CustomPropertyReference,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    references.push(&reference.name);
    if let Some(fallback) = reference.fallback.as_deref() {
        collect_custom_references(fallback, references);
    }
}

fn collect_length_percentage_references<'a>(
    value: &'a LengthPercentageValue,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    if let LengthPercentageValue::Calc(expression) = value {
        collect_calc_references(expression, references);
    }
}

fn collect_calc_references<'a>(
    expression: &'a CalcExpression,
    references: &mut Vec<&'a CustomPropertyName>,
) {
    match expression {
        CalcExpression::Variable(reference) => collect_reference(reference, references),
        CalcExpression::Value(value) => collect_length_percentage_references(value, references),
        CalcExpression::Add(left, right)
        | CalcExpression::Sub(left, right)
        | CalcExpression::Mul(left, right)
        | CalcExpression::Div(left, right) => {
            collect_calc_references(left, references);
            collect_calc_references(right, references);
        }
        CalcExpression::Number(_) => {}
    }
}

fn resolve_custom_name(
    name: &CustomPropertyName,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    if let Some(cached) = cache.get(name) {
        return cached.clone();
    }
    if cyclic.contains(name) || visiting.iter().any(|candidate| candidate == name) {
        cache.insert(name.clone(), None);
        return None;
    }
    visiting.push(name.clone());
    let resolved = candidates.get(name).and_then(|value| {
        resolve_value_from_candidates(value, candidates, cyclic, cache, visiting)
    });
    visiting.pop();
    cache.insert(name.clone(), resolved.clone());
    resolved
}

fn resolve_reference_from_candidates(
    reference: &CustomPropertyReference,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    resolve_custom_name(&reference.name, candidates, cyclic, cache, visiting).or_else(|| {
        reference.fallback.as_deref().and_then(|fallback| {
            resolve_value_from_candidates(fallback, candidates, cyclic, cache, visiting)
        })
    })
}

fn resolve_reference_from_computed(
    reference: &CustomPropertyReference,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<StyleValue> {
    computed.get(&reference.name).cloned().or_else(|| {
        reference
            .fallback
            .as_deref()
            .and_then(|fallback| resolve_value_from_computed(fallback, computed))
    })
}

fn resolve_value_from_candidates(
    value: &StyleValue,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<StyleValue> {
    match value {
        StyleValue::Variable(reference) => {
            resolve_reference_from_candidates(reference, candidates, cyclic, cache, visiting)
        }
        value => {
            let mut resolve = |value: &LengthPercentageValue| {
                resolve_length_percentage_from_candidates(
                    value, candidates, cyclic, cache, visiting,
                )
            };
            map_nested_length_percentages(value, &mut resolve)
        }
    }
}

fn resolve_value_from_computed(
    value: &StyleValue,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<StyleValue> {
    match value {
        StyleValue::Variable(reference) => resolve_reference_from_computed(reference, computed),
        value => {
            let mut resolve = |value: &LengthPercentageValue| {
                resolve_length_percentage_from_computed(value, computed)
            };
            map_nested_length_percentages(value, &mut resolve)
        }
    }
}

fn map_nested_length_percentages(
    value: &StyleValue,
    resolve: &mut dyn FnMut(&LengthPercentageValue) -> Option<LengthPercentageValue>,
) -> Option<StyleValue> {
    crate::value_tree::try_map_length_percentages(value, resolve)
}

fn resolve_length_percentage_from_candidates(
    value: &LengthPercentageValue,
    candidates: &BTreeMap<CustomPropertyName, StyleValue>,
    cyclic: &BTreeSet<CustomPropertyName>,
    cache: &mut BTreeMap<CustomPropertyName, Option<StyleValue>>,
    visiting: &mut Vec<CustomPropertyName>,
) -> Option<LengthPercentageValue> {
    resolve_length_percentage_with(value, &mut |reference| {
        resolve_reference_from_candidates(reference, candidates, cyclic, cache, visiting)
    })
}

fn resolve_length_percentage_from_computed(
    value: &LengthPercentageValue,
    computed: &BTreeMap<CustomPropertyName, StyleValue>,
) -> Option<LengthPercentageValue> {
    resolve_length_percentage_with(value, &mut |reference| {
        resolve_reference_from_computed(reference, computed)
    })
}

fn resolve_length_percentage_with(
    value: &LengthPercentageValue,
    resolve_reference: &mut dyn FnMut(&CustomPropertyReference) -> Option<StyleValue>,
) -> Option<LengthPercentageValue> {
    match value {
        LengthPercentageValue::Calc(expression) => Some(LengthPercentageValue::Calc(Box::new(
            resolve_calc_with(expression, resolve_reference)?,
        ))),
        value => Some(value.clone()),
    }
}

fn resolve_calc_with(
    expression: &CalcExpression,
    resolve_reference: &mut dyn FnMut(&CustomPropertyReference) -> Option<StyleValue>,
) -> Option<CalcExpression> {
    match expression {
        CalcExpression::Variable(reference) => {
            resolve_reference(reference).and_then(style_value_to_calc)
        }
        CalcExpression::Value(value) => Some(CalcExpression::Value(Box::new(
            resolve_length_percentage_with(value, resolve_reference)?,
        ))),
        CalcExpression::Number(value) => Some(CalcExpression::Number(*value)),
        CalcExpression::Add(left, right) => Some(CalcExpression::Add(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Sub(left, right) => Some(CalcExpression::Sub(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Mul(left, right) => Some(CalcExpression::Mul(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
        CalcExpression::Div(left, right) => Some(CalcExpression::Div(
            Box::new(resolve_calc_with(left, resolve_reference)?),
            Box::new(resolve_calc_with(right, resolve_reference)?),
        )),
    }
}

fn style_value_to_calc(value: StyleValue) -> Option<CalcExpression> {
    match value {
        StyleValue::Number(value) => Some(CalcExpression::Number(value)),
        StyleValue::Length(value) => Some(CalcExpression::Value(Box::new(
            LengthPercentageValue::Length(value),
        ))),
        StyleValue::LengthPercentage(LengthPercentageValue::Calc(expression)) => Some(*expression),
        StyleValue::LengthPercentage(value) => Some(CalcExpression::Value(Box::new(value))),
        _ => None,
    }
}

/// Compatibility name for callers that initially consumed only text styles.
///
/// The returned [`ComputedStyle`] also includes every implemented layout
/// property. New code may use [`resolve_style`] for a name matching that scope.
pub fn resolve_text_style(
    specified: &SpecifiedStyle,
    parent: Option<&InheritedStyle>,
    environment: StyleEnvironment,
) -> Result<ResolvedNodeStyle, StyleResolutionError> {
    resolve_style(specified, parent, environment)
}

#[derive(Default)]
struct InheritedDeclarations<'a> {
    cursor: Option<&'a StyleValue>,
    pointer_events: Option<&'a StyleValue>,
    font_family: Option<&'a StyleValue>,
    font_features: Option<&'a StyleValue>,
    font_variations: Option<&'a StyleValue>,
    font_optical_sizing: Option<&'a StyleValue>,
    font_size: Option<&'a StyleValue>,
    font_weight: Option<&'a StyleValue>,
    font_style: Option<&'a StyleValue>,
    line_height: Option<&'a StyleValue>,
    letter_spacing: Option<&'a StyleValue>,
    color: Option<&'a StyleValue>,
    text_align: Option<&'a StyleValue>,
    text_decoration: Option<&'a StyleValue>,
    text_shadow: Option<&'a StyleValue>,
}

impl<'a> InheritedDeclarations<'a> {
    fn from_specified(specified: &'a SpecifiedStyle) -> Self {
        let mut values = Self::default();
        for declaration in specified.resolved() {
            let slot = match declaration.property() {
                StyleProperty::Cursor => &mut values.cursor,
                StyleProperty::PointerEvents => &mut values.pointer_events,
                StyleProperty::FontFamily => &mut values.font_family,
                StyleProperty::FontFeatureSettings => &mut values.font_features,
                StyleProperty::FontVariationSettings => &mut values.font_variations,
                StyleProperty::FontOpticalSizing => &mut values.font_optical_sizing,
                StyleProperty::FontSize => &mut values.font_size,
                StyleProperty::FontWeight => &mut values.font_weight,
                StyleProperty::FontStyle => &mut values.font_style,
                StyleProperty::LineHeight => &mut values.line_height,
                StyleProperty::LetterSpacing => &mut values.letter_spacing,
                StyleProperty::Color => &mut values.color,
                StyleProperty::TextAlign => &mut values.text_align,
                StyleProperty::TextDecoration => &mut values.text_decoration,
                StyleProperty::TextShadow => &mut values.text_shadow,
                _ => continue,
            };
            *slot = Some(declaration.value());
        }
        values
    }
}

fn canonical_features(values: &[FontFeatureValue]) -> Vec<FontFeatureValue> {
    values
        .iter()
        .fold(BTreeMap::new(), |mut result, value| {
            result.insert(value.tag, value.value);
            result
        })
        .into_iter()
        .map(|(tag, value)| FontFeatureValue { tag, value })
        .collect()
}

fn canonical_variations(
    values: &[FontVariationValue],
) -> Result<Vec<FontVariationValue>, StyleResolutionError> {
    let mut result = BTreeMap::new();
    for value in values {
        if !value.value.get().is_finite() {
            return Err(StyleResolutionError::InvalidPropertyValue(
                StyleProperty::FontVariationSettings,
            ));
        }
        result.insert(value.tag, value.value);
    }
    Ok(result
        .into_iter()
        .map(|(tag, value)| FontVariationValue { tag, value })
        .collect())
}

fn expect_length_percentage(
    property: StyleProperty,
    value: &StyleValue,
) -> Result<&LengthPercentageValue, StyleResolutionError> {
    match value {
        StyleValue::LengthPercentage(value) => Ok(value),
        _ => Err(wrong_type(property)),
    }
}

fn wrong_type(property: StyleProperty) -> StyleResolutionError {
    StyleResolutionError::InvalidPropertyValue(property)
}

fn finite(value: StyleNumber, property: StyleProperty) -> Result<f32, StyleResolutionError> {
    let value = value.get();
    if value.is_finite() {
        Ok(value)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

fn resolve_length(
    value: LengthValue,
    em_basis: f32,
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
                LengthUnit::Em => em_basis,
                LengthUnit::Rem => environment.root_font_size(),
                LengthUnit::Vh => environment.viewport_height() / 100.0,
                LengthUnit::Vw => environment.viewport_width() / 100.0,
            };
            (finite(value, property)?, multiplier)
        }
    };
    let resolved = number * multiplier;
    if resolved.is_finite() {
        Ok(resolved)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

fn resolve_length_percentage(
    value: &LengthPercentageValue,
    percentage_basis: f32,
    em_basis: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<f32, StyleResolutionError> {
    let resolved = match value {
        LengthPercentageValue::Length(value) => {
            resolve_length(*value, em_basis, environment, property)?
        }
        LengthPercentageValue::Percentage(value) => {
            finite(*value, property)? * percentage_basis / 100.0
        }
        LengthPercentageValue::Calc(expression) => {
            match evaluate_calc(
                expression,
                percentage_basis,
                em_basis,
                environment,
                property,
            )? {
                Quantity::Length(value) => value,
                Quantity::Number(_) => {
                    return Err(StyleResolutionError::InvalidCalculation(property));
                }
            }
        }
    };
    if resolved.is_finite() {
        Ok(resolved)
    } else {
        Err(StyleResolutionError::InvalidPropertyValue(property))
    }
}

#[derive(Clone, Copy, Debug)]
enum Quantity {
    Number(f32),
    Length(f32),
}

fn evaluate_calc(
    expression: &CalcExpression,
    percentage_basis: f32,
    em_basis: f32,
    environment: StyleEnvironment,
    property: StyleProperty,
) -> Result<Quantity, StyleResolutionError> {
    let invalid = || StyleResolutionError::InvalidCalculation(property);
    match expression {
        CalcExpression::Value(value) => {
            resolve_length_percentage(value, percentage_basis, em_basis, environment, property)
                .map(Quantity::Length)
        }
        CalcExpression::Number(value) => finite(*value, property).map(Quantity::Number),
        CalcExpression::Variable(_) => Err(invalid()),
        CalcExpression::Add(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left + right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Length(left + right))
                }
                _ => Err(invalid()),
            }
        }
        CalcExpression::Sub(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left - right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Length(left - right))
                }
                _ => Err(invalid()),
            }
        }
        CalcExpression::Mul(left, right) => {
            match (
                evaluate_calc(left, percentage_basis, em_basis, environment, property)?,
                evaluate_calc(right, percentage_basis, em_basis, environment, property)?,
            ) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left * right))
                }
                (Quantity::Number(number), Quantity::Length(length))
                | (Quantity::Length(length), Quantity::Number(number)) => {
                    Ok(Quantity::Length(number * length))
                }
                (Quantity::Length(_), Quantity::Length(_)) => Err(invalid()),
            }
        }
        CalcExpression::Div(left, right) => {
            let left = evaluate_calc(left, percentage_basis, em_basis, environment, property)?;
            let right = evaluate_calc(right, percentage_basis, em_basis, environment, property)?;
            match (left, right) {
                (_, Quantity::Number(0.0)) | (_, Quantity::Length(0.0)) => Err(invalid()),
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Ok(Quantity::Number(left / right))
                }
                (Quantity::Length(left), Quantity::Number(right)) => {
                    Ok(Quantity::Length(left / right))
                }
                (Quantity::Length(left), Quantity::Length(right)) => {
                    Ok(Quantity::Number(left / right))
                }
                (Quantity::Number(_), Quantity::Length(_)) => Err(invalid()),
            }
        }
    }
}

fn normalize_color(value: &ColorValue) -> Result<ColorValue, StyleResolutionError> {
    normalize_color_for(value, StyleProperty::Color)
}

pub(crate) fn normalize_color_for(
    value: &ColorValue,
    property: StyleProperty,
) -> Result<ColorValue, StyleResolutionError> {
    let invalid = || StyleResolutionError::InvalidPropertyValue(property);
    match value {
        ColorValue::Named(name) if name.is_empty() => Err(invalid()),
        ColorValue::Named(name) => Ok(ColorValue::Named(name.clone())),
        ColorValue::Rgba {
            red,
            green,
            blue,
            alpha,
        } => {
            let alpha = finite(*alpha, property)?;
            if !(0.0..=1.0).contains(&alpha) {
                return Err(invalid());
            }
            Ok(ColorValue::Rgba {
                red: *red,
                green: *green,
                blue: *blue,
                alpha: StyleNumber::new(alpha),
            })
        }
        ColorValue::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => {
            let hue = finite(*hue_degrees, property)?.rem_euclid(360.0);
            let saturation = finite(*saturation, property)?;
            let lightness = finite(*lightness, property)?;
            let alpha = finite(*alpha, property)?;
            if !(0.0..=100.0).contains(&saturation)
                || !(0.0..=100.0).contains(&lightness)
                || !(0.0..=1.0).contains(&alpha)
            {
                return Err(invalid());
            }
            Ok(ColorValue::Hsla {
                hue_degrees: StyleNumber::new(hue),
                saturation: StyleNumber::new(saturation),
                lightness: StyleNumber::new(lightness),
                alpha: StyleNumber::new(alpha),
            })
        }
    }
}

#[cfg(test)]
mod tests {
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
                    value: number(3.0),
                    unit: LengthUnit::Rpx,
                },
                3.0,
            ),
            (
                LengthValue::Dimension {
                    value: number(4.0),
                    unit: LengthUnit::Ppx,
                },
                2.0,
            ),
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
                StyleValue::LengthPercentage(LengthPercentageValue::Length(
                    LengthValue::Dimension {
                        value: number(2.0),
                        unit: LengthUnit::Em,
                    },
                )),
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
                offset_x: px(1.0),
                offset_y: LengthValue::Dimension {
                    value: number(1.0),
                    unit: LengthUnit::Em,
                },
                blur_radius: px(3.0),
                color: ColorValue::Named("red".into()),
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
                offset_x: LengthValue::Zero,
                offset_y: LengthValue::Zero,
                blur_radius: px(-1.0),
                color: ColorValue::Named("black".into()),
            }),
        );
        assert_eq!(
            resolve_text_style(&invalid, None, environment).unwrap_err(),
            StyleResolutionError::InvalidPropertyValue(StyleProperty::TextShadow)
        );

        let invalid_offset_x = declaration(
            StyleProperty::TextShadow,
            StyleValue::TextShadow(TextShadowValue::Shadow {
                offset_x: px(f32::NAN),
                offset_y: LengthValue::Zero,
                blur_radius: LengthValue::Zero,
                color: ColorValue::Named("black".into()),
            }),
        );
        let invalid_offset_y = declaration(
            StyleProperty::TextShadow,
            StyleValue::TextShadow(TextShadowValue::Shadow {
                offset_x: LengthValue::Zero,
                offset_y: px(f32::NAN),
                blur_radius: LengthValue::Zero,
                color: ColorValue::Named("black".into()),
            }),
        );
        let invalid_blur = declaration(
            StyleProperty::TextShadow,
            StyleValue::TextShadow(TextShadowValue::Shadow {
                offset_x: LengthValue::Zero,
                offset_y: LengthValue::Zero,
                blur_radius: px(f32::NAN),
                color: ColorValue::Named("black".into()),
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
                offset_x: LengthValue::Zero,
                offset_y: LengthValue::Zero,
                blur_radius: LengthValue::Zero,
                color: ColorValue::Rgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: number(f32::NAN),
                },
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
                color: Some(ColorValue::Rgba {
                    red: 0,
                    green: 0,
                    blue: 0,
                    alpha: number(f32::NAN),
                }),
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
                                Box::new(CalcExpression::Variable(CustomPropertyReference::new(
                                    name,
                                ))),
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
                StyleValue::LengthPercentageAuto(
                    crate::LengthPercentageAutoValue::LengthPercentage(length_percentage()),
                ),
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
                        angle_degrees: number(180.0),
                        stops: vec![
                            crate::GradientStopValue {
                                color: ColorValue::Named("red".into()),
                                position: Some(nested()),
                            },
                            crate::GradientStopValue {
                                color: ColorValue::Named("blue".into()),
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
}
