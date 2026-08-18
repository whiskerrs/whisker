//! Typed declaration storage and deterministic fragment composition.

use std::collections::HashSet;

use crate::{StyleProperty, StyleValue};

/// One typed inline-style declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StyleDeclaration {
    property: StyleProperty,
    value: StyleValue,
}

impl StyleDeclaration {
    /// Creates a declaration for a registered common property.
    pub const fn new(property: StyleProperty, value: StyleValue) -> Self {
        Self { property, value }
    }

    /// Returns the stable property identity.
    pub const fn property(&self) -> StyleProperty {
        self.property
    }

    /// Returns the semantic value.
    pub const fn value(&self) -> &StyleValue {
        &self.value
    }

    /// Splits the declaration into its property and value.
    pub fn into_parts(self) -> (StyleProperty, StyleValue) {
        (self.property, self.value)
    }
}

/// An ordered set of explicitly specified inline-style declarations.
///
/// Repeated properties remain in insertion history and resolve with the last
/// declaration winning. This makes fragment composition deterministic without
/// selectors, specificity, or a global cascade.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SpecifiedStyle {
    declarations: Vec<StyleDeclaration>,
}

impl SpecifiedStyle {
    /// Creates an empty style.
    pub const fn new() -> Self {
        Self {
            declarations: Vec::new(),
        }
    }

    /// Appends a declaration.
    pub fn push(mut self, property: StyleProperty, value: StyleValue) -> Self {
        self.declarations
            .push(StyleDeclaration::new(property, value));
        self
    }

    /// Appends another fragment so its declarations override earlier writes.
    pub fn merge(mut self, other: Self) -> Self {
        self.declarations.extend(other.declarations);
        self
    }

    /// Returns whether the fragment has no declarations.
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    /// Returns the number of declarations including overridden writes.
    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    /// Iterates over insertion history.
    pub fn declarations(&self) -> impl Iterator<Item = &StyleDeclaration> {
        self.declarations.iter()
    }

    /// Iterates over the last declaration for each property in final-write
    /// order.
    pub fn resolved(&self) -> Vec<&StyleDeclaration> {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for declaration in self.declarations.iter().rev() {
            if seen.insert(declaration.property) {
                resolved.push(declaration);
            }
        }
        resolved.reverse();
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LengthValue, StyleNumber};

    #[test]
    fn declaration_accessors_and_parts_preserve_types() {
        let declaration = StyleDeclaration::new(
            StyleProperty::Opacity,
            StyleValue::Number(StyleNumber::new(0.5)),
        );
        assert_eq!(declaration.property(), StyleProperty::Opacity);
        assert_eq!(
            declaration.value(),
            &StyleValue::Number(StyleNumber::new(0.5))
        );
        assert_eq!(
            declaration.into_parts(),
            (
                StyleProperty::Opacity,
                StyleValue::Number(StyleNumber::new(0.5))
            )
        );
    }

    #[test]
    fn empty_style_reports_empty() {
        let style = SpecifiedStyle::new();
        assert!(style.is_empty());
        assert_eq!(style.len(), 0);
        assert_eq!(style.declarations().count(), 0);
        assert!(style.resolved().is_empty());
    }

    #[test]
    fn resolution_is_last_wins_in_final_write_order() {
        let style = SpecifiedStyle::new()
            .push(
                StyleProperty::Opacity,
                StyleValue::Number(StyleNumber::new(0.2)),
            )
            .push(StyleProperty::Width, StyleValue::Length(LengthValue::Zero))
            .push(
                StyleProperty::Opacity,
                StyleValue::Number(StyleNumber::new(0.8)),
            );
        assert_eq!(style.len(), 3);
        let resolved = style.resolved();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].property(), StyleProperty::Width);
        assert_eq!(resolved[1].property(), StyleProperty::Opacity);
        assert_eq!(
            resolved[1].value(),
            &StyleValue::Number(StyleNumber::new(0.8))
        );
    }

    #[test]
    fn merge_appends_the_overriding_fragment() {
        let base = SpecifiedStyle::new().push(
            StyleProperty::Opacity,
            StyleValue::Number(StyleNumber::new(0.1)),
        );
        let overlay = SpecifiedStyle::new().push(
            StyleProperty::Opacity,
            StyleValue::Number(StyleNumber::new(0.9)),
        );
        let merged = base.merge(overlay);
        assert_eq!(merged.declarations().count(), 2);
        assert_eq!(
            merged.resolved()[0].value(),
            &StyleValue::Number(StyleNumber::new(0.9))
        );
    }
}
