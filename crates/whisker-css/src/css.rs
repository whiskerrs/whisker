//! The [`Css`] container and its internal [`CssProp`] entries.
//!
//! Every typed builder method on [`Css`] records a stable [`StyleProperty`]
//! identity and temporarily resolves its argument to CSS text via [`ToCss`].
//! Shorthand methods expand to their constituent longhands where possible, so
//! the canonical last-write-wins rule applies per longhand
//! property — calling `.padding(px(8)).padding_top(px(0))` leaves
//! `padding-top: 0px; padding-right: 8px; padding-bottom: 8px;
//! padding-left: 8px;`, exactly as a CSS author would expect.

use core::fmt;

use crate::style_value::ToStyleValue;
use crate::to_css::ToCss;
use whisker_style::{
    CustomPropertyName, CustomPropertyReference, SpecifiedStyle, StyleProperty, StylePropertyId,
    StyleValue,
};

/// A declaration still requiring the temporary Lynx CSS compatibility path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnmigratedStyleValue {
    property: String,
}

impl UnmigratedStyleValue {
    /// Returns the compatibility property that has no semantic value yet.
    pub fn property(&self) -> &str {
        &self.property
    }
}

impl fmt::Display for UnmigratedStyleValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "style property `{}` still requires Lynx CSS compatibility",
            self.property
        )
    }
}

impl std::error::Error for UnmigratedStyleValue {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertyKey {
    Known(StyleProperty),
    Legacy(&'static str),
    Custom(CustomPropertyName),
}

impl PropertyKey {
    fn from_name(name: &'static str) -> Self {
        StyleProperty::from_css_name(name).map_or(Self::Legacy(name), Self::Known)
    }

    fn name(&self) -> &str {
        match self {
            Self::Known(property) => property.css_name(),
            Self::Legacy(name) => name,
            Self::Custom(name) => name.as_str(),
        }
    }
}

/// One CSS declaration stored inside a [`Css`].
///
/// Constructed only by [`Css`]'s builder methods; the internal
/// representation is intentionally opaque so the crate is free to
/// switch to a typed enum without breaking callers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CssProp {
    property: PropertyKey,
    style_value: Option<StyleValue>,
    lynx_value: String,
}

impl CssProp {
    /// Build a property from a CSS name and an already-serialized
    /// value. Crate-public; users should go through [`Css`].
    pub(crate) fn new(property: StyleProperty, value: String) -> Self {
        Self {
            property: PropertyKey::Known(property),
            style_value: None,
            lynx_value: value,
        }
    }

    pub(crate) fn typed(
        property: StyleProperty,
        style_value: StyleValue,
        lynx_value: String,
    ) -> Self {
        Self {
            property: PropertyKey::Known(property),
            style_value: Some(style_value),
            lynx_value,
        }
    }

    fn legacy(name: &'static str, value: String) -> Self {
        Self {
            property: PropertyKey::from_name(name),
            style_value: None,
            lynx_value: value,
        }
    }

    fn custom(name: CustomPropertyName, value: StyleValue, css_value: String) -> Self {
        Self {
            property: PropertyKey::Custom(name),
            style_value: Some(value),
            lynx_value: css_value,
        }
    }

    /// The CSS property name (`"padding-top"`, `"background-color"`).
    pub fn name(&self) -> &str {
        self.property.name()
    }

    /// The registered property identity, or `None` for an unknown name added
    /// through the temporary [`Css::raw`] migration escape hatch.
    pub fn property(&self) -> Option<StyleProperty> {
        match &self.property {
            PropertyKey::Known(property) => Some(*property),
            PropertyKey::Legacy(_) | PropertyKey::Custom(_) => None,
        }
    }

    /// The stable common-property ID, or `None` for an unknown legacy name.
    pub fn property_id(&self) -> Option<StylePropertyId> {
        self.property().map(StyleProperty::id)
    }

    /// The semantic value, or `None` while this declaration still uses the
    /// compatibility-only Lynx CSS representation.
    pub fn style_value(&self) -> Option<&StyleValue> {
        self.style_value.as_ref()
    }

    /// The serialized CSS value (`"8px"`, `"rgb(26, 26, 46)"`).
    pub fn value(&self) -> &str {
        &self.lynx_value
    }
}

impl ToCss for CssProp {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(self.name())?;
        dest.write_str(": ")?;
        dest.write_str(&self.lynx_value)?;
        dest.write_char(';')
    }
}

/// A type-safe CSS style declaration block.
///
/// Build a style by chaining builder methods; every method returns
/// `Self` so further calls can be appended fluently. The resulting
/// CSS text is produced by [`ToCss::to_css_string`] or via
/// [`Display`](core::fmt::Display).
///
/// ```ignore
/// use whisker_css::ext::*;
/// use whisker_css::{Color, Display, FlexDirection, Css};
///
/// let s = Css::new()
///     .display(Display::Flex)
///     .flex_direction(FlexDirection::Column)
///     .padding(px(12))
///     .background_color(Color::hex(0x1A1A2E));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Css {
    props: Vec<CssProp>,
}

impl Css {
    /// An empty style.
    pub fn new() -> Self {
        Self { props: Vec::new() }
    }

    /// Push a property, taking ownership of `self` to return it. All
    /// public builder methods funnel through this helper.
    pub(crate) fn push(mut self, property: StyleProperty, value: impl ToCss) -> Self {
        self.props
            .push(CssProp::new(property, value.to_css_string()));
        self
    }

    /// Pushes a semantic value while retaining its temporary Lynx spelling.
    pub(crate) fn push_typed<T>(mut self, property: StyleProperty, value: T) -> Self
    where
        T: ToCss + ToStyleValue,
    {
        let lynx_value = value.to_css_string();
        self.props
            .push(CssProp::typed(property, value.to_style_value(), lynx_value));
        self
    }

    /// Pushes an already-normalized semantic value and its migration-only Lynx
    /// spelling.
    pub(crate) fn push_semantic(
        mut self,
        property: StyleProperty,
        style_value: StyleValue,
        lynx_value: impl Into<String>,
    ) -> Self {
        self.props
            .push(CssProp::typed(property, style_value, lynx_value.into()));
        self
    }

    /// Push a property whose value is an already-serialized string.
    pub(crate) fn push_raw(mut self, property: StyleProperty, value: impl Into<String>) -> Self {
        self.props.push(CssProp::new(property, value.into()));
        self
    }

    /// Escape hatch — append a raw CSS declaration without
    /// type-checking. Use this when Lynx supports a property Whisker
    /// has not yet wrapped, or when copying a value verbatim from
    /// hand-written CSS.
    ///
    /// `name` should be a `&'static str` because property names are
    /// part of the CSS grammar, not runtime data. The value is taken
    /// verbatim and not validated.
    pub fn raw(self, name: &'static str, value: impl Into<String>) -> Self {
        let mut this = self;
        this.props.push(CssProp::legacy(name, value.into()));
        this
    }

    /// Defines an inherited typed CSS custom property.
    ///
    /// Custom properties are case-sensitive and use their standard `--name`
    /// spelling. Whisker retains the semantic value rather than reparsing CSS
    /// text during rendering.
    pub fn custom_property<T>(mut self, name: CustomPropertyName, value: T) -> Self
    where
        T: CustomPropertyValue,
    {
        let css_value = value.to_css_string();
        self.props.push(CssProp::custom(
            name,
            value.to_custom_style_value(),
            css_value,
        ));
        self
    }

    /// Sets a registered property from `var(--name)`.
    ///
    /// Type compatibility is checked by computed-style resolution, matching
    /// CSS's computed-value-time behavior for custom properties.
    pub fn property_variable(self, property: StyleProperty, name: CustomPropertyName) -> Self {
        let css = format!("var({})", name.as_str());
        self.push_semantic(
            property,
            StyleValue::Variable(CustomPropertyReference::new(name)),
            css,
        )
    }

    /// Sets a registered property from `var(--name, <fallback>)`.
    pub fn property_variable_with_fallback<T>(
        self,
        property: StyleProperty,
        name: CustomPropertyName,
        fallback: T,
    ) -> Self
    where
        T: CustomPropertyValue,
    {
        let css = format!("var({}, {})", name.as_str(), fallback.to_css_string());
        self.push_semantic(
            property,
            StyleValue::Variable(CustomPropertyReference::with_fallback(
                name,
                fallback.to_custom_style_value(),
            )),
            css,
        )
    }

    /// True if no declarations have been added.
    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
    }

    /// Number of declarations currently in the style. Repeats of the
    /// same property are counted separately; they collapse during
    /// serialization.
    pub fn len(&self) -> usize {
        self.props.len()
    }

    /// Iterate over every entry in insertion order, including
    /// duplicates of the same property. Use [`Self::resolved`] for
    /// last-write-wins iteration.
    pub fn entries(&self) -> impl Iterator<Item = &CssProp> {
        self.props.iter()
    }

    /// Iterate over entries with the last-write-wins rule applied:
    /// only the final occurrence of each property name is yielded,
    /// in the position of that final occurrence.
    pub fn resolved(&self) -> Vec<&CssProp> {
        let mut seen: std::collections::HashSet<PropertyKey> = std::collections::HashSet::new();
        let mut out: Vec<&CssProp> = Vec::new();
        for prop in self.props.iter().rev() {
            if seen.insert(prop.property.clone()) {
                out.push(prop);
            }
        }
        out.reverse();
        out
    }

    /// Extend by appending every entry of `other`. Later writes win
    /// during serialization, so `.merge(other)` lets `other` override
    /// declarations already set on `self`.
    pub fn merge(mut self, other: Css) -> Self {
        self.props.extend(other.props);
        self
    }

    /// Converts this authoring fragment to renderer-independent typed storage.
    ///
    /// Migration-only declarations fail with the first property that still
    /// depends on Lynx CSS text. No CSS parsing is performed.
    pub fn to_specified_style(&self) -> Result<SpecifiedStyle, UnmigratedStyleValue> {
        let mut style = SpecifiedStyle::new();
        for property in &self.props {
            let Some(value) = property.style_value.clone() else {
                return Err(UnmigratedStyleValue {
                    property: property.name().to_owned(),
                });
            };
            match &property.property {
                PropertyKey::Known(property_id) => {
                    style = style.push(*property_id, value);
                }
                PropertyKey::Custom(name) => {
                    style = style.push_custom(name.clone(), value);
                }
                PropertyKey::Legacy(_) => {
                    return Err(UnmigratedStyleValue {
                        property: property.name().to_owned(),
                    });
                }
            }
        }
        Ok(style)
    }
}

/// A typed authoring value that can be stored in a CSS custom property.
pub trait CustomPropertyValue: ToCss {
    /// Converts to renderer-independent specified style.
    #[doc(hidden)]
    fn to_custom_style_value(&self) -> StyleValue;
}

impl<T> CustomPropertyValue for T
where
    T: ToCss + ToStyleValue,
{
    fn to_custom_style_value(&self) -> StyleValue {
        self.to_style_value()
    }
}

impl ToCss for Css {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        let resolved = self.resolved();
        for (i, prop) in resolved.iter().enumerate() {
            if i > 0 {
                dest.write_char(' ')?;
            }
            prop.to_css(dest)?;
        }
        Ok(())
    }
}

impl fmt::Display for Css {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        ToCss::to_css(self, f)
    }
}

impl From<Css> for String {
    fn from(s: Css) -> Self {
        s.to_css_string()
    }
}

impl From<&Css> for String {
    fn from(s: &Css) -> Self {
        s.to_css_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CalcExpr, Color, Display, FlexDirection, FontStyle, FontWeight, Length, LengthPercentage,
        LineHeight, NamedColor, Percentage, Size,
    };

    #[test]
    fn empty_style_serializes_to_empty_string() {
        assert_eq!(Css::new().to_css_string(), "");
        assert!(Css::new().is_empty());
    }

    #[test]
    fn raw_appends_a_declaration() {
        let s = Css::new().raw("color", "red");
        assert_eq!(s.to_css_string(), "color: red;");
        assert!(!s.is_empty());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn multiple_distinct_properties_keep_order() {
        let s = Css::new()
            .raw("color", "red")
            .raw("background-color", "blue");
        assert_eq!(s.to_css_string(), "color: red; background-color: blue;");
    }

    #[test]
    fn duplicate_property_uses_last_value() {
        let s = Css::new()
            .raw("color", "red")
            .raw("color", "blue")
            .raw("color", "green");
        assert_eq!(s.to_css_string(), "color: green;");
        assert_eq!(s.len(), 3);
        assert_eq!(s.resolved().len(), 1);
    }

    #[test]
    fn duplicate_property_preserves_position_of_last() {
        // The last write decides where the property lands in the
        // resolved order.
        let s = Css::new()
            .raw("color", "red")
            .raw("background-color", "white")
            .raw("color", "blue");
        assert_eq!(s.to_css_string(), "background-color: white; color: blue;");
    }

    #[test]
    fn entries_iterates_all_in_order() {
        let s = Css::new().raw("color", "red").raw("color", "blue");
        let names: Vec<&str> = s.entries().map(|p| p.name()).collect();
        assert_eq!(names, ["color", "color"]);
    }

    #[test]
    fn merge_lets_other_win() {
        let base = Css::new().raw("color", "red");
        let overlay = Css::new().raw("color", "blue");
        let merged = base.merge(overlay);
        assert_eq!(merged.to_css_string(), "color: blue;");
    }

    #[test]
    fn merge_preserves_distinct_props() {
        let base = Css::new().raw("color", "red");
        let overlay = Css::new().raw("background-color", "yellow");
        let merged = base.merge(overlay);
        assert_eq!(
            merged.to_css_string(),
            "color: red; background-color: yellow;"
        );
    }

    #[test]
    fn into_string_via_from_owned() {
        let s = Css::new().raw("color", "red");
        let css: String = s.into();
        assert_eq!(css, "color: red;");
    }

    #[test]
    fn into_string_via_from_borrowed() {
        let s = Css::new().raw("color", "red");
        let css: String = (&s).into();
        assert_eq!(css, "color: red;");
    }

    #[test]
    fn display_matches_to_css_string() {
        let s = Css::new().raw("color", "red").raw("padding", "8px");
        assert_eq!(format!("{s}"), s.to_css_string());
    }

    #[test]
    fn style_prop_accessors() {
        let s = Css::new().raw("color", "red");
        let prop = s.entries().next().unwrap();
        assert_eq!(prop.name(), "color");
        assert_eq!(prop.property(), Some(StyleProperty::Color));
        assert_eq!(prop.property_id(), Some(StyleProperty::Color.id()));
        assert_eq!(prop.style_value(), None);
        assert_eq!(prop.value(), "red");
        assert_eq!(prop.to_css_string(), "color: red;");
    }

    #[test]
    fn unknown_raw_property_has_no_registered_identity() {
        let s = Css::new().raw("future-property", "value");
        let prop = s.entries().next().unwrap();
        assert_eq!(prop.name(), "future-property");
        assert_eq!(prop.property(), None);
        assert_eq!(prop.property_id(), None);
    }

    #[test]
    fn known_raw_and_typed_writes_share_the_same_slot() {
        let s = Css::new()
            .push(StyleProperty::Color, Token("red"))
            .raw("color", "blue");
        assert_eq!(s.resolved().len(), 1);
        assert_eq!(s.to_css_string(), "color: blue;");
    }

    #[test]
    fn typed_push_keeps_semantics_separate_from_lynx_text() {
        let s = Css::new().push_typed(StyleProperty::PaddingTop, Length::Px(8.0));
        let prop = s.entries().next().unwrap();
        assert_eq!(
            prop.style_value(),
            Some(&StyleValue::Length(whisker_style::LengthValue::Dimension {
                value: whisker_style::StyleNumber::new(8.0),
                unit: whisker_style::LengthUnit::Px,
            }))
        );
        assert_eq!(prop.value(), "8px");
    }

    #[test]
    fn typed_fragment_converts_without_parsing_css() {
        let css = Css::new().padding_top(Length::Px(8.0));
        let style = css.to_specified_style().unwrap();
        assert_eq!(style.len(), 1);
        assert_eq!(style.resolved()[0].property(), StyleProperty::PaddingTop);
    }

    #[test]
    fn compatibility_fragment_reports_the_blocking_property() {
        let error = Css::new()
            .raw("future-property", "value")
            .to_specified_style()
            .unwrap_err();
        assert_eq!(error.property(), "future-property");
        assert_eq!(
            error.to_string(),
            "style property `future-property` still requires Lynx CSS compatibility"
        );
    }

    #[test]
    fn typed_custom_property_round_trips_and_resolves_without_css_parsing() {
        let spacing = CustomPropertyName::new("--spacing").unwrap();
        let css = Css::new()
            .custom_property(spacing.clone(), Size::from(Length::Px(24.0)))
            .property_variable(StyleProperty::Width, spacing);

        assert_eq!(
            css.to_css_string(),
            "--spacing: 24px; width: var(--spacing);"
        );
        let specified = css.to_specified_style().unwrap();
        assert_eq!(specified.len(), 2);
        let resolved = whisker_style::resolve_style(
            &specified,
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().layout().size.width,
            whisker_style::ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(
                24.0, 0.0
            ))
        );
    }

    #[test]
    fn typed_custom_property_composes_inside_calc_without_css_parsing() {
        let gap = CustomPropertyName::new("--gap").unwrap();
        let width = LengthPercentage::calc(
            CalcExpr::variable(gap.clone()).add(CalcExpr::value(Length::Px(8.0))),
        );
        let css = Css::new()
            .custom_property(gap, Length::Px(12.0))
            .width(width);

        assert_eq!(
            css.to_css_string(),
            "--gap: 12px; width: calc(var(--gap) + 8px);"
        );
        let resolved = whisker_style::resolve_style(
            &css.to_specified_style().unwrap(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().layout().size.width,
            whisker_style::ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(
                20.0, 0.0,
            ))
        );
    }

    #[test]
    fn inherited_typed_custom_property_composes_inside_child_calc() {
        let gap = CustomPropertyName::new("--gap").unwrap();
        let parent = Css::new().custom_property(gap.clone(), Length::Px(12.0));
        let parent = whisker_style::resolve_style(
            &parent.to_specified_style().unwrap(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        let child = Css::new().width(LengthPercentage::calc(
            CalcExpr::variable(gap).add(CalcExpr::value(Length::Px(8.0))),
        ));
        let child = whisker_style::resolve_style(
            &child.to_specified_style().unwrap(),
            Some(parent.inherited_for_children()),
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();

        assert_eq!(
            child.computed().layout().size.width,
            whisker_style::ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(
                20.0, 0.0,
            ))
        );
    }

    #[test]
    fn calc_variable_fallback_survives_a_nested_custom_property_cycle() {
        let a = CustomPropertyName::new("--a").unwrap();
        let b = CustomPropertyName::new("--b").unwrap();
        let a_value = LengthPercentage::calc(
            CalcExpr::variable(b.clone()).add(CalcExpr::value(Length::Px(1.0))),
        );
        let b_value = LengthPercentage::calc(
            CalcExpr::variable(a.clone()).add(CalcExpr::value(Length::Px(1.0))),
        );
        let width = LengthPercentage::calc(
            CalcExpr::variable_with_fallback(a.clone(), CalcExpr::value(Length::Px(5.0)))
                .add(CalcExpr::value(Length::Px(3.0))),
        );
        let css = Css::new()
            .custom_property(a, a_value)
            .custom_property(b, b_value)
            .width(width);

        let resolved = whisker_style::resolve_style(
            &css.to_specified_style().unwrap(),
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.computed().layout().size.width,
            whisker_style::ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(
                8.0, 0.0,
            ))
        );
    }

    #[test]
    fn property_variable_fallback_is_semantic_and_serializable() {
        let missing = CustomPropertyName::new("--missing").unwrap();
        let css = Css::new().property_variable_with_fallback(
            StyleProperty::Color,
            missing,
            Color::Named(NamedColor::Red),
        );
        assert_eq!(css.to_css_string(), "color: var(--missing, red);");
        let specified = css.to_specified_style().unwrap();
        let resolved = whisker_style::resolve_style(
            &specified,
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        assert_eq!(
            resolved.inherited_for_children().color(),
            &whisker_style::ColorValue::Named("red".into())
        );
    }

    #[test]
    fn inherited_text_fragment_resolves_entirely_in_rust() {
        let css = Css::new()
            .font_family("Inter")
            .font_size(Length::Px(20.0))
            .font_weight(FontWeight::Bold)
            .font_style(FontStyle::Italic)
            .line_height(LineHeight::Number(1.5))
            .letter_spacing(Length::Px(1.0))
            .color(Color::Named(NamedColor::Red));
        let specified = css.to_specified_style().unwrap();
        let resolved = whisker_style::resolve_text_style(
            &specified,
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        let text = resolved.inherited_for_children();
        assert_eq!(text.font_size(), 20.0);
        assert_eq!(text.font_weight(), whisker_style::FontWeightValue::BOLD);
        assert_eq!(text.font_style(), whisker_style::FontStyleValue::Italic);
        assert_eq!(text.letter_spacing(), 1.0);
        assert_eq!(
            text.line_height(),
            whisker_style::ComputedLineHeight::LogicalPixels(whisker_style::StyleNumber::new(30.0))
        );
    }

    #[test]
    fn box_and_flex_fragment_resolves_entirely_in_rust() {
        let specified = Css::new()
            .display(Display::Flex)
            .flex_direction(FlexDirection::Column)
            .width(Length::Px(120.0))
            .row_gap(Percentage(10.0))
            .to_specified_style()
            .unwrap();
        let resolved = whisker_style::resolve_style(
            &specified,
            None,
            whisker_style::StyleEnvironment::default(),
        )
        .unwrap();
        let layout = resolved.computed().layout();
        assert_eq!(layout.display, whisker_style::DisplayValue::Flex);
        assert_eq!(
            layout.flex_direction,
            whisker_style::FlexDirectionValue::Column
        );
        assert_eq!(
            layout.size.width,
            whisker_style::ComputedSizeValue::Value(whisker_style::ComputedLengthPercentage::new(
                120.0, 0.0
            ))
        );
        assert_eq!(layout.gap.height.length(), 0.0);
        assert_eq!(layout.gap.height.fraction(), 0.1);
    }

    struct Token(&'static str);

    impl ToCss for Token {
        fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
            dest.write_str(self.0)
        }
    }
}
