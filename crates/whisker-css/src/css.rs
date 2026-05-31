//! The [`Css`] container and its internal [`CssProp`] entries.
//!
//! Every typed builder method on [`Css`] resolves its argument to
//! CSS text via [`ToCss`] and pushes a [`CssProp`] onto an internal
//! list. Shorthand methods expand to their constituent longhands so
//! the canonical last-write-wins rule applies per longhand
//! property — calling `.padding(px(8)).padding_top(px(0))` leaves
//! `padding-top: 0px; padding-right: 8px; padding-bottom: 8px;
//! padding-left: 8px;`, exactly as a CSS author would expect.

use core::fmt;

use crate::to_css::ToCss;

/// One CSS declaration stored inside a [`Css`].
///
/// Constructed only by [`Css`]'s builder methods; the internal
/// representation is intentionally opaque so the crate is free to
/// switch to a typed enum without breaking callers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CssProp {
    name: &'static str,
    value: String,
}

impl CssProp {
    /// Build a property from a CSS name and an already-serialized
    /// value. Crate-public; users should go through [`Css`].
    pub(crate) fn new(name: &'static str, value: String) -> Self {
        Self { name, value }
    }

    /// The CSS property name (`"padding-top"`, `"background-color"`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The serialized CSS value (`"8px"`, `"rgb(26, 26, 46)"`).
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl ToCss for CssProp {
    fn to_css(&self, dest: &mut dyn fmt::Write) -> fmt::Result {
        dest.write_str(self.name)?;
        dest.write_str(": ")?;
        dest.write_str(&self.value)?;
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
        Self {
            props: Vec::new(),
        }
    }

    /// Push a property, taking ownership of `self` to return it. All
    /// public builder methods funnel through this helper.
    pub(crate) fn push(mut self, name: &'static str, value: impl ToCss) -> Self {
        self.props.push(CssProp::new(name, value.to_css_string()));
        self
    }

    /// Push a property whose value is an already-serialized string.
    pub(crate) fn push_raw(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.props.push(CssProp::new(name, value.into()));
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
        self.push_raw(name, value)
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
        // Walk backwards, recording the first time we see each name,
        // then reverse for forward order.
        let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        let mut out: Vec<&CssProp> = Vec::new();
        for prop in self.props.iter().rev() {
            if seen.insert(prop.name) {
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
        assert_eq!(
            s.to_css_string(),
            "color: red; background-color: blue;"
        );
    }

    #[test]
    fn duplicate_property_uses_last_value() {
        let s = Css::new()
            .raw("color", "red")
            .raw("color", "blue")
            .raw("color", "green");
        assert_eq!(s.to_css_string(), "color: green;");
        // Internal `entries` keeps all three — only `resolved`
        // collapses.
        assert_eq!(s.len(), 3);
        assert_eq!(s.resolved().len(), 1);
    }

    #[test]
    fn duplicate_property_preserves_position_of_last() {
        // color appears at index 0 then again at 2; final order
        // should place `color: blue` where the last occurrence sits
        // (after `background-color`).
        let s = Css::new()
            .raw("color", "red")
            .raw("background-color", "white")
            .raw("color", "blue");
        assert_eq!(
            s.to_css_string(),
            "background-color: white; color: blue;"
        );
    }

    #[test]
    fn entries_iterates_all_in_order() {
        let s = Css::new()
            .raw("color", "red")
            .raw("color", "blue");
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
        assert_eq!(prop.value(), "red");
        assert_eq!(prop.to_css_string(), "color: red;");
    }
}
