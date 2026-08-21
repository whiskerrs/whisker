//! Rust-side definition of Whisker's built-in primitive element module.

use crate::{ElementModuleDefinition, ElementProviderMetadata, ElementTag};
use whisker_engine::whisker_protocol::{
    ElementChildMount, ElementContentKind, ElementMeasurement, ElementSchema,
};

/// Canonical key for the standard presentation container.
pub const VIEW_ELEMENT_NAME: &str = "whisker.ui/View";
/// Canonical key for the standard plain-text element.
pub const TEXT_ELEMENT_NAME: &str = "whisker.ui/Text";
/// Canonical key for the standard scroll container.
pub const SCROLL_VIEW_ELEMENT_NAME: &str = "whisker.ui/ScrollView";

/// Returns the providers exported by the built-in UI package.
///
/// Hosts consume these providers exactly like application-selected external
/// element modules; there is no separately maintained tag-to-schema table.
pub fn standard_element_providers() -> Vec<ElementProviderMetadata> {
    vec![view(), text(), scroll_view()]
}

/// Returns the Rust-side module definition for `whisker.ui`.
pub fn standard_ui_module_definition() -> ElementModuleDefinition {
    ElementModuleDefinition::new("whisker.ui", standard_element_providers())
}

fn view() -> ElementProviderMetadata {
    ElementProviderMetadata::builtin(
        ElementTag::View,
        ElementSchema {
            canonical_name: VIEW_ELEMENT_NAME.into(),
            content: ElementContentKind::None,
            child_mount: ElementChildMount::Presentation,
            measurement: ElementMeasurement::None,
            consumes_text_style: false,
        },
    )
}

fn text() -> ElementProviderMetadata {
    ElementProviderMetadata::builtin(
        ElementTag::Text,
        ElementSchema {
            canonical_name: TEXT_ELEMENT_NAME.into(),
            content: ElementContentKind::Text,
            child_mount: ElementChildMount::None,
            measurement: ElementMeasurement::Text,
            consumes_text_style: true,
        },
    )
}

fn scroll_view() -> ElementProviderMetadata {
    ElementProviderMetadata::builtin(
        ElementTag::ScrollView,
        ElementSchema {
            canonical_name: SCROLL_VIEW_ELEMENT_NAME.into(),
            content: ElementContentKind::ScrollContainer,
            child_mount: ElementChildMount::ScrollContent,
            measurement: ElementMeasurement::None,
            consumes_text_style: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ui_is_an_ordinary_element_provider_module() {
        let definition = standard_ui_module_definition();
        assert_eq!(definition.module_name, "whisker.ui");
        assert_eq!(definition.elements.len(), 3);
        assert_eq!(
            definition.elements[0].schema.canonical_name,
            "whisker.ui/View"
        );
        assert_eq!(
            definition.elements[1].schema.canonical_name,
            "whisker.ui/Text"
        );
        assert_eq!(
            definition.elements[2].schema.canonical_name,
            "whisker.ui/ScrollView"
        );
    }
}
