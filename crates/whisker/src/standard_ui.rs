//! Rust-side definition of Whisker's built-in primitive element module.

use crate::{ElementModuleDefinition, ElementProviderMetadata, ElementSchema, ElementTag};
use whisker_engine::whisker_style::{
    FlexDirectionValue, OverflowValue, SpecifiedStyle, StyleProperty, StyleValue,
};

#[whisker::builtin_component(
    name = "whisker.ui/View",
    measurement = None,
)]
fn view(style: crate::Style, children: crate::runtime::view::Children) {}

#[whisker::builtin_component(
    name = "whisker.ui/Text",
    measurement = Text,
)]
fn text(style: crate::Style, children: crate::TextChildren) {}

#[whisker::builtin_component(
    name = "whisker.ui/ScrollView",
    measurement = None,
)]
fn scroll_view(style: crate::Style, children: crate::runtime::view::Children) {}

/// Stable name for the standard presentation container.
pub const VIEW_ELEMENT_NAME: &str = view_schema::NAME;
/// Stable name for the standard plain-text element.
pub const TEXT_ELEMENT_NAME: &str = text_schema::NAME;
/// Stable name for the standard scroll container.
pub const SCROLL_VIEW_ELEMENT_NAME: &str = scroll_view_schema::NAME;

/// Returns the Host-independent binding for the standard presentation
/// container. Hosts pair this value with their own native factory.
pub fn view_element_binding() -> ElementSchema {
    view_schema::schema()
}

/// Returns the Host-independent binding for the standard text element.
pub fn text_element_binding() -> ElementSchema {
    text_schema::schema()
}

/// Returns the Host-independent binding for the standard scroll container.
pub fn scroll_view_element_binding() -> ElementSchema {
    scroll_view_schema::schema()
}

/// Returns the providers exported by the built-in UI package.
///
/// Core consumes this authoring metadata when building the standard registry.
/// Hosts consume the binding functions above and therefore never depend on
/// built-in `ElementTag` mappings. Each schema comes from the shared
/// declaration compiler.
pub(crate) fn standard_element_providers() -> Vec<ElementProviderMetadata> {
    vec![
        ElementProviderMetadata::builtin(ElementTag::View, view_element_binding()),
        ElementProviderMetadata::builtin(ElementTag::Text, text_element_binding()),
        ElementProviderMetadata::builtin(ElementTag::ScrollView, scroll_view_element_binding())
            .with_base_style(scroll_view_base_style()),
    ]
}

fn scroll_view_base_style() -> SpecifiedStyle {
    // A vertical ScrollView is a bounded viewport, not a row-shaped content
    // container. Hidden layout overflow gives Taffy the CSS scroll-container
    // automatic-minimum behavior; each native Host still owns scrolling.
    SpecifiedStyle::new()
        .push(
            StyleProperty::FlexDirection,
            StyleValue::FlexDirection(FlexDirectionValue::Column),
        )
        .push(
            StyleProperty::OverflowX,
            StyleValue::Overflow(OverflowValue::Hidden),
        )
        .push(
            StyleProperty::OverflowY,
            StyleValue::Overflow(OverflowValue::Hidden),
        )
}

/// Returns the Rust-side module definition for `whisker.ui`.
pub(crate) fn standard_ui_module_definition() -> ElementModuleDefinition {
    ElementModuleDefinition::new("whisker.ui", standard_element_providers())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ui_is_an_ordinary_element_provider_module() {
        let definition = standard_ui_module_definition();
        assert_eq!(definition.module_name, "whisker.ui");
        assert_eq!(definition.elements.len(), 3);
        assert_eq!(definition.elements[0].schema.name, "whisker.ui/View");
        assert_eq!(definition.elements[1].schema.name, "whisker.ui/Text");
        assert_eq!(definition.elements[2].schema.name, "whisker.ui/ScrollView");
        assert_eq!(
            definition.elements[0].schema.child_policy,
            crate::ChildPolicy::Elements
        );
        assert_eq!(
            definition.elements[1].schema.child_policy,
            crate::ChildPolicy::PlainText
        );
        assert_eq!(
            definition.elements[2].schema.child_policy,
            crate::ChildPolicy::Elements
        );
        assert_eq!(
            definition.elements[1].schema.measurement,
            crate::ElementMeasurement::Text
        );
        assert_eq!(definition.elements[0].schema, view_element_binding());
        assert_eq!(definition.elements[1].schema, text_element_binding());
        assert_eq!(definition.elements[2].schema, scroll_view_element_binding());
        assert!(!definition.elements[2].base_style.is_empty());
        assert_eq!(
            definition.elements[0].authoring,
            crate::ElementAuthoringBinding::Builtin(ElementTag::View)
        );
    }
}
