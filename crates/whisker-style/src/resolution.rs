//! Deterministic resolution for Whisker's fixed inherited text context.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BorderStyleValue, CalcExpression, ColorValue, ComponentValue, ComputedLayoutStyle,
    ComputedLengthPercentage, ComputedMotionStyle, ComputedPaintStyle, CursorValue,
    CustomPropertyName, CustomPropertyReference, DirectionValue, FontFamilyValue, FontFeatureValue,
    FontOpticalSizingValue, FontStyleValue, FontVariationValue, FontWeightValue,
    LengthPercentageValue, LengthUnit, LengthValue, LineHeightValue, PointerEventsValue,
    SpecifiedStyle, StyleNumber, StyleProperty, StyleValue, TextAlignValue,
    TextDecorationLineValue, TextDecorationStyleValue, TextDecorationValue, TextOverflowValue,
    TextShadowValue, WhiteSpaceValue, WordBreakValue,
};

mod custom_properties;
mod values;

use custom_properties::*;
#[cfg(test)]
use values::evaluate_calc;
pub(crate) use values::normalize_color_for;
use values::*;

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
    /// Mixed fixed and percentage components produced by `calc()`.
    LengthPercentage {
        /// Fixed logical-pixel component.
        logical_pixels: StyleNumber,
        /// Percentage number before the `%` suffix.
        percentage: StyleNumber,
    },
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
    let mut variable_properties = specified
        .resolved()
        .into_iter()
        .filter_map(|declaration| {
            let mut references = Vec::new();
            collect_custom_references(declaration.value(), &mut references);
            (!references.is_empty()).then_some(declaration.property())
        })
        .collect::<BTreeSet<_>>();
    let mut effective = specified.clone();
    loop {
        match resolve_style_once(&effective, parent, environment) {
            Err(
                StyleResolutionError::InvalidPropertyValue(property)
                | StyleResolutionError::InvalidCalculation(property),
            ) if variable_properties.remove(&property) => {
                effective = without_registered_property(&effective, property);
            }
            result => return result,
        }
    }
}

fn without_registered_property(
    specified: &SpecifiedStyle,
    rejected: StyleProperty,
) -> SpecifiedStyle {
    let mut filtered = SpecifiedStyle::new();
    for declaration in specified.declarations() {
        if declaration.property() != rejected {
            filtered = filtered.push(declaration.property(), declaration.value().clone());
        }
    }
    for declaration in specified.custom_declarations() {
        filtered = filtered.push_custom(declaration.name().clone(), declaration.value().clone());
    }
    filtered
}

fn resolve_style_once(
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
                .map(|color| {
                    normalize_color_for(resolved_component(color), StyleProperty::TextDecoration)
                })
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
                *resolved_component(offset_x),
                font_size.get(),
                environment,
                StyleProperty::TextShadow,
            )?;
            let offset_y = resolve_length(
                *resolved_component(offset_y),
                font_size.get(),
                environment,
                StyleProperty::TextShadow,
            )?;
            let blur_radius = resolve_length(
                *resolved_component(blur_radius),
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
                color: normalize_color_for(resolved_component(color), StyleProperty::TextShadow)?,
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
        Some(StyleValue::LengthPercentage(value @ LengthPercentageValue::Calc(_))) => {
            let value = crate::layout::resolve_affine(
                value,
                font_size.get(),
                environment,
                StyleProperty::TextIndent,
            )?;
            let logical_pixels = value.length();
            let percentage = value.fraction() * 100.0;
            if percentage == 0.0 {
                ComputedTextIndent::LogicalPixels(StyleNumber::new(logical_pixels))
            } else if logical_pixels == 0.0 {
                ComputedTextIndent::Percentage(StyleNumber::new(percentage))
            } else {
                ComputedTextIndent::LengthPercentage {
                    logical_pixels: StyleNumber::new(logical_pixels),
                    percentage: StyleNumber::new(percentage),
                }
            }
        }
        Some(_) => return Err(wrong_type(StyleProperty::TextIndent)),
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

    let mut layout = crate::layout::resolve_layout_style(
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
    if matches!(
        paint.border_styles.top,
        BorderStyleValue::None | BorderStyleValue::Hidden
    ) {
        layout.border.top = ComputedLengthPercentage::ZERO;
    }
    if matches!(
        paint.border_styles.right,
        BorderStyleValue::None | BorderStyleValue::Hidden
    ) {
        layout.border.right = ComputedLengthPercentage::ZERO;
    }
    if matches!(
        paint.border_styles.bottom,
        BorderStyleValue::None | BorderStyleValue::Hidden
    ) {
        layout.border.bottom = ComputedLengthPercentage::ZERO;
    }
    if matches!(
        paint.border_styles.left,
        BorderStyleValue::None | BorderStyleValue::Hidden
    ) {
        layout.border.left = ComputedLengthPercentage::ZERO;
    }
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

#[cfg(test)]
mod tests;
