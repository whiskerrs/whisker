//! Host-independent definitions of Whisker's built-in primitive elements.

use crate::element::ElementTag;
use crate::{ElementModuleDefinition, ElementProviderMetadata};
use whisker_engine::whisker_protocol::{
    ChildPolicy, CommandId, ElementCommandSchema, ElementEventSchema, ElementMeasurement,
    ElementPropertySchema, ElementSchema, ElementValueKind, EventId, PropertyId,
};
use whisker_engine::whisker_style::{
    FlexDirectionValue, OverflowValue, SpecifiedStyle, StyleProperty, StyleValue,
};

/// Stable name for the standard presentation container.
pub const VIEW_ELEMENT_NAME: &str = "whisker.ui/View";
/// Stable name for the standard plain-text element.
pub const TEXT_ELEMENT_NAME: &str = "whisker.ui/Text";
/// Stable name for the standard scroll container.
pub const SCROLL_VIEW_ELEMENT_NAME: &str = "whisker.ui/ScrollView";

/// Stable property ID for the ScrollView's logical scroll axis.
pub const SCROLL_ORIENTATION_PROPERTY: PropertyId =
    PropertyId::new(1).expect("the standard scroll orientation property ID is non-zero");
/// Stable property ID for item-aligned scroll settling.
pub const ITEM_SNAP_PROPERTY: PropertyId =
    PropertyId::new(2).expect("the standard item snap property ID is non-zero");
/// Stable property ID for whether scrolling may pass intermediate snap points.
pub const SCROLL_SNAP_STOP_PROPERTY: PropertyId =
    PropertyId::new(3).expect("the standard scroll snap stop property ID is non-zero");
/// Stable property ID controlling user-driven ScrollView gestures.
pub const SCROLL_ENABLED_PROPERTY: PropertyId =
    PropertyId::new(4).expect("the standard scroll enabled property ID is non-zero");
/// Stable command ID for absolute ScrollView movement.
pub const SCROLL_TO_COMMAND: CommandId =
    CommandId::new(1).expect("the standard scrollTo command ID is non-zero");
/// Stable command ID for relative ScrollView movement.
pub const SCROLL_BY_COMMAND: CommandId =
    CommandId::new(2).expect("the standard scrollBy command ID is non-zero");

fn element_schema(
    name: &'static str,
    child_policy: ChildPolicy,
    measurement: ElementMeasurement,
) -> ElementSchema {
    ElementSchema {
        name: name.to_owned(),
        child_policy,
        measurement,
        text_style: false,
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
    let mut schema = element_schema(
        SCROLL_VIEW_ELEMENT_NAME,
        ChildPolicy::Elements,
        ElementMeasurement::None,
    );
    schema.properties.extend([
        ElementPropertySchema {
            property: SCROLL_ORIENTATION_PROPERTY,
            name: "scroll-orientation".to_owned(),
            value: ElementValueKind::String,
        },
        ElementPropertySchema {
            property: ITEM_SNAP_PROPERTY,
            name: "item-snap".to_owned(),
            value: ElementValueKind::Map,
        },
        ElementPropertySchema {
            property: SCROLL_SNAP_STOP_PROPERTY,
            name: "scroll-snap-stop".to_owned(),
            value: ElementValueKind::String,
        },
        ElementPropertySchema {
            property: SCROLL_ENABLED_PROPERTY,
            name: "enable-scroll".to_owned(),
            value: ElementValueKind::Bool,
        },
    ]);
    schema.events.push(ElementEventSchema {
        event: EventId::new(1).expect("the standard scroll event ID is non-zero"),
        name: "scroll".to_owned(),
        detail: Some(ElementValueKind::Map),
    });
    schema.commands.extend([
        ElementCommandSchema {
            command: SCROLL_TO_COMMAND,
            name: "scrollTo".to_owned(),
            arguments: ElementValueKind::Map,
        },
        ElementCommandSchema {
            command: SCROLL_BY_COMMAND,
            name: "scrollBy".to_owned(),
            arguments: ElementValueKind::Map,
        },
    ]);
    schema
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
        assert_eq!(definition.elements[2].schema.properties.len(), 4);
        assert_eq!(
            definition.elements[2].schema.properties[0].name,
            "scroll-orientation"
        );
        assert_eq!(
            definition.elements[2].schema.properties[1].name,
            "item-snap"
        );
        assert_eq!(
            definition.elements[2].schema.properties[2].name,
            "scroll-snap-stop"
        );
        assert_eq!(
            definition.elements[2].schema.properties[3].name,
            "enable-scroll"
        );
        assert!(!definition.elements[2].base_style.is_empty());
        assert_eq!(
            definition.elements[0].authoring,
            ElementAuthoringBinding::Builtin(ElementTag::View)
        );
    }
}
