//! Host-independent definitions of Whisker's built-in primitive elements.

use crate::element::ElementTag;
use crate::{ElementModuleDefinition, ElementProviderMetadata};
use whisker_engine::whisker_protocol::{ChildPolicy, ElementMeasurement, ElementSchema};
use whisker_engine::whisker_style::{
    FlexDirectionValue, OverflowValue, SpecifiedStyle, StyleProperty, StyleValue,
};

/// Stable name for the standard presentation container.
pub const VIEW_ELEMENT_NAME: &str = "whisker.ui/View";
/// Stable name for the standard plain-text element.
pub const TEXT_ELEMENT_NAME: &str = "whisker.ui/Text";
/// Stable name for the standard scroll container.
pub const SCROLL_VIEW_ELEMENT_NAME: &str = "whisker.ui/ScrollView";

fn element_schema(
    name: &'static str,
    child_policy: ChildPolicy,
    measurement: ElementMeasurement,
) -> ElementSchema {
    ElementSchema {
        name: name.to_owned(),
        child_policy,
        measurement,
        properties: Vec::new(),
        events: Vec::new(),
        commands: Vec::new(),
    }
}

/// Returns the Host-independent binding for the standard presentation
/// container. Hosts pair this value with their own native factory.
pub fn view_element_binding() -> ElementSchema {
    element_schema(
        VIEW_ELEMENT_NAME,
        ChildPolicy::Elements,
        ElementMeasurement::None,
    )
}

/// Returns the Host-independent binding for the standard text element.
pub fn text_element_binding() -> ElementSchema {
    element_schema(
        TEXT_ELEMENT_NAME,
        ChildPolicy::PlainText,
        ElementMeasurement::Text,
    )
}

/// Returns the Host-independent binding for the standard scroll container.
pub fn scroll_view_element_binding() -> ElementSchema {
    element_schema(
        SCROLL_VIEW_ELEMENT_NAME,
        ChildPolicy::Elements,
        ElementMeasurement::None,
    )
}

/// Returns the providers exported by the built-in UI package.
pub(crate) fn standard_element_providers() -> Vec<ElementProviderMetadata> {
    vec![
        ElementProviderMetadata::builtin(ElementTag::View, view_element_binding()),
        ElementProviderMetadata::builtin(ElementTag::Text, text_element_binding()),
        ElementProviderMetadata::builtin(ElementTag::ScrollView, scroll_view_element_binding())
            .with_base_style(scroll_view_base_style()),
    ]
}

fn scroll_view_base_style() -> SpecifiedStyle {
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
    use crate::ElementAuthoringBinding;

    #[test]
    fn standard_ui_is_an_ordinary_element_provider_module() {
        let definition = standard_ui_module_definition();
        assert_eq!(definition.module_name, "whisker.ui");
        assert_eq!(definition.elements.len(), 3);
        assert_eq!(definition.elements[0].schema.name, VIEW_ELEMENT_NAME);
        assert_eq!(definition.elements[1].schema.name, TEXT_ELEMENT_NAME);
        assert_eq!(definition.elements[2].schema.name, SCROLL_VIEW_ELEMENT_NAME);
        assert_eq!(
            definition.elements[0].schema.child_policy,
            ChildPolicy::Elements
        );
        assert_eq!(
            definition.elements[1].schema.child_policy,
            ChildPolicy::PlainText
        );
        assert_eq!(
            definition.elements[2].schema.child_policy,
            ChildPolicy::Elements
        );
        assert_eq!(
            definition.elements[1].schema.measurement,
            ElementMeasurement::Text
        );
        assert_eq!(definition.elements[0].schema, view_element_binding());
        assert_eq!(definition.elements[1].schema, text_element_binding());
        assert_eq!(definition.elements[2].schema, scroll_view_element_binding());
        assert!(!definition.elements[2].base_style.is_empty());
        assert_eq!(
            definition.elements[0].authoring,
            ElementAuthoringBinding::Builtin(ElementTag::View)
        );
    }
}
