//! Element schemas negotiated before a surface presents its first frame.

use std::collections::HashSet;

use crate::{CommandId, ElementTypeId, EventId, PropertyId, WhiskerValue};

/// Top-level wire value accepted by an element property or command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementValueKind {
    /// Explicit absence or an argument-free command.
    Null,
    /// Boolean state.
    Bool,
    /// Signed integer state.
    Int,
    /// Floating-point state.
    Float,
    /// UTF-8 string state.
    String,
    /// Opaque bytes.
    Bytes,
    /// Ordered values.
    Array,
    /// Named fields.
    Map,
}

impl ElementValueKind {
    /// Returns whether a protocol value has this top-level shape.
    pub fn accepts(self, value: &WhiskerValue) -> bool {
        value.is_data()
            && matches!(
                (self, value),
                (Self::Null, WhiskerValue::Null)
                    | (Self::Bool, WhiskerValue::Bool(_))
                    | (Self::Int, WhiskerValue::Int(_))
                    | (Self::Float, WhiskerValue::Float(_))
                    | (Self::String, WhiskerValue::String(_))
                    | (Self::Bytes, WhiskerValue::Bytes(_))
                    | (Self::Array, WhiskerValue::Array(_))
                    | (Self::Map, WhiskerValue::Map(_))
            )
    }
}

/// One typed property exported by an element schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementPropertySchema {
    /// Stable generated identifier within this element contract.
    pub property: PropertyId,
    /// Authoring and diagnostic name.
    pub name: String,
    /// Accepted top-level value shape.
    pub value: ElementValueKind,
}

/// One node-scoped event exported by an element schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementEventSchema {
    /// Stable generated identifier within this element contract.
    pub event: EventId,
    /// Authoring and diagnostic name.
    pub name: String,
    /// Optional top-level payload constraint. `None` accepts any
    /// [`WhiskerValue`] data variant.
    pub detail: Option<ElementValueKind>,
}

impl ElementEventSchema {
    /// Returns this event's bit in [`Operation::SetEventMask`](crate::Operation::SetEventMask).
    pub fn mask(&self) -> Option<u64> {
        // EventId reserves zero, so converting from its one-based wire ID cannot
        // underflow. Keeping that invariant explicit also avoids suggesting that
        // a zero ID can enter schema validation.
        let shift = self.event.get() - 1;
        (shift < u64::BITS).then(|| 1_u64 << shift)
    }

    /// Returns whether `detail` is transferable module data and satisfies the
    /// optional top-level payload constraint.
    pub fn accepts_detail(&self, detail: &WhiskerValue) -> bool {
        detail.is_data() && self.detail.is_none_or(|expected| expected.accepts(detail))
    }
}

/// One imperative command exported by an element schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementCommandSchema {
    /// Stable generated identifier within this element contract.
    pub command: CommandId,
    /// Authoring and diagnostic name.
    pub name: String,
    /// Accepted top-level argument shape.
    pub arguments: ElementValueKind,
}

/// Host-independent element contract before a surface assigns compact IDs.
///
/// Generated module metadata and built-in primitives both enter bootstrap in
/// this form. The resulting surface registry turns each schema into an
/// [`ElementRegistration`] with an immutable [`ElementTypeId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementSchema {
    /// Stable, versionless package-qualified name.
    pub name: String,
    /// Which kind of authoring children core may lower for this element.
    ///
    /// The Host never receives a heterogeneous child value. Core lowers
    /// element children to tree operations and plain-text children to
    /// [`Operation::SetText`](crate::Operation::SetText). Where element
    /// children mount remains a Host-local implementation detail.
    pub child_policy: ChildPolicy,
    /// Intrinsic measurement behavior.
    pub measurement: ElementMeasurement,
    /// Whether the Host content implementation consumes resolved inherited
    /// text style independently from plain-text content.
    pub text_style: bool,
    /// Typed element-specific properties.
    pub properties: Vec<ElementPropertySchema>,
    /// Typed node-scoped events.
    pub events: Vec<ElementEventSchema>,
    /// Typed imperative commands.
    pub commands: Vec<ElementCommandSchema>,
}

impl ElementSchema {
    /// Validates schema invariants independent of a particular Host backend.
    pub fn validate(&self) -> Result<(), ElementRegistrationError> {
        validate_contract(&self.name)?;
        validate_members(&self.properties, &self.events, &self.commands)
    }

    /// Binds this schema to a compact ID allocated for one registry epoch.
    pub fn bind(self, element_type: ElementTypeId) -> ElementRegistration {
        ElementRegistration {
            element_type,
            name: self.name,
            child_policy: self.child_policy,
            measurement: self.measurement,
            text_style: self.text_style,
            properties: self.properties,
            events: self.events,
            commands: self.commands,
        }
    }
}

/// Authoring child model accepted by an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChildPolicy {
    /// The element is a leaf and accepts no authoring children.
    None,
    /// Children are ordinary scene elements lowered to insert/move/remove ops.
    Elements,
    /// Children are plain-text fragments normalized into one text-content op.
    PlainText,
}

impl ChildPolicy {
    /// Returns whether ordinary scene elements may be inserted below the node.
    pub const fn accepts_elements(self) -> bool {
        matches!(self, Self::Elements)
    }

    /// Returns whether raw authoring text may be lowered for the node.
    pub const fn accepts_plain_text(self) -> bool {
        matches!(self, Self::PlainText)
    }
}

/// Intrinsic measurement provider selected by an element schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementMeasurement {
    /// Taffy derives size from style and children without Host measurement.
    None,
    /// The common Host text shaper supplies intrinsic metrics.
    Text,
    /// Rust-side resource metadata or a replaced-content provider supplies size.
    ReplacedContent,
    /// An element-specific platform measurer supplies intrinsic metrics.
    Custom,
}

/// One element contract bound to a compact type ID for a surface registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementRegistration {
    /// Compact identifier carried by frame and measurement packets.
    pub element_type: ElementTypeId,
    /// Stable, versionless package-qualified name.
    pub name: String,
    /// Child model negotiated for this element.
    pub child_policy: ChildPolicy,
    /// Intrinsic measurement behavior.
    pub measurement: ElementMeasurement,
    /// Whether the Host content implementation consumes resolved inherited
    /// text style independently from plain-text content.
    pub text_style: bool,
    /// Typed element-specific properties.
    pub properties: Vec<ElementPropertySchema>,
    /// Typed node-scoped events.
    pub events: Vec<ElementEventSchema>,
    /// Typed imperative commands.
    pub commands: Vec<ElementCommandSchema>,
}

impl ElementRegistration {
    /// Validates schema invariants independent of a particular Host backend.
    pub fn validate(&self) -> Result<(), ElementRegistrationError> {
        validate_contract(&self.name)?;
        validate_members(&self.properties, &self.events, &self.commands)
    }

    /// Returns the Host-independent schema represented by this registration.
    pub fn schema(&self) -> ElementSchema {
        ElementSchema {
            name: self.name.clone(),
            child_policy: self.child_policy,
            measurement: self.measurement,
            text_style: self.text_style,
            properties: self.properties.clone(),
            events: self.events.clone(),
            commands: self.commands.clone(),
        }
    }

    /// Resolves a property authoring name.
    pub fn property_named(&self, name: &str) -> Option<&ElementPropertySchema> {
        self.properties
            .iter()
            .find(|property| property.name == name)
    }

    /// Resolves a property identifier.
    pub fn property(&self, id: PropertyId) -> Option<&ElementPropertySchema> {
        self.properties
            .iter()
            .find(|property| property.property == id)
    }

    /// Resolves an event authoring name.
    pub fn event_named(&self, name: &str) -> Option<&ElementEventSchema> {
        self.events.iter().find(|event| event.name == name)
    }

    /// Resolves an event identifier.
    pub fn event(&self, id: EventId) -> Option<&ElementEventSchema> {
        self.events.iter().find(|event| event.event == id)
    }

    /// Resolves a command authoring name.
    pub fn command_named(&self, name: &str) -> Option<&ElementCommandSchema> {
        self.commands.iter().find(|command| command.name == name)
    }

    /// Resolves a command identifier.
    pub fn command(&self, id: CommandId) -> Option<&ElementCommandSchema> {
        self.commands.iter().find(|command| command.command == id)
    }
}

fn validate_members(
    properties: &[ElementPropertySchema],
    events: &[ElementEventSchema],
    commands: &[ElementCommandSchema],
) -> Result<(), ElementRegistrationError> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for property in properties {
        if property.name.trim().is_empty() {
            return Err(ElementRegistrationError::EmptyMemberName);
        }
        if !ids.insert(property.property) {
            return Err(ElementRegistrationError::DuplicatePropertyId);
        }
        if !names.insert(property.name.as_str()) {
            return Err(ElementRegistrationError::DuplicatePropertyName);
        }
    }
    let mut event_ids = HashSet::new();
    names.clear();
    for event in events {
        if event.name.trim().is_empty() {
            return Err(ElementRegistrationError::EmptyMemberName);
        }
        if event.mask().is_none() {
            return Err(ElementRegistrationError::EventIdOutsideMask);
        }
        if !event_ids.insert(event.event) {
            return Err(ElementRegistrationError::DuplicateEventId);
        }
        if !names.insert(event.name.as_str()) {
            return Err(ElementRegistrationError::DuplicateEventName);
        }
    }
    let mut command_ids = HashSet::new();
    names.clear();
    for command in commands {
        if command.name.trim().is_empty() {
            return Err(ElementRegistrationError::EmptyMemberName);
        }
        if !command_ids.insert(command.command) {
            return Err(ElementRegistrationError::DuplicateCommandId);
        }
        if !names.insert(command.name.as_str()) {
            return Err(ElementRegistrationError::DuplicateCommandName);
        }
    }
    Ok(())
}

fn validate_contract(name: &str) -> Result<(), ElementRegistrationError> {
    if name.trim().is_empty() {
        return Err(ElementRegistrationError::EmptyName);
    }
    Ok(())
}

/// Invalid Host-independent element registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementRegistrationError {
    /// The package-qualified element name was empty.
    EmptyName,
    /// A property, event, or command name was empty.
    EmptyMemberName,
    /// Two properties declared the same ID.
    DuplicatePropertyId,
    /// Two properties declared the same name.
    DuplicatePropertyName,
    /// Two events declared the same ID.
    DuplicateEventId,
    /// Two events declared the same name.
    DuplicateEventName,
    /// An event ID could not fit in the v1 64-bit event mask.
    EventIdOutsideMask,
    /// Two commands declared the same ID.
    DuplicateCommandId,
    /// Two commands declared the same name.
    DuplicateCommandName,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property(id: u32, name: &str) -> ElementPropertySchema {
        ElementPropertySchema {
            property: PropertyId::new(id).unwrap(),
            name: name.into(),
            value: ElementValueKind::Bool,
        }
    }

    fn event(id: u32, name: &str) -> ElementEventSchema {
        ElementEventSchema {
            event: EventId::new(id).unwrap(),
            name: name.into(),
            detail: None,
        }
    }

    fn command(id: u32, name: &str) -> ElementCommandSchema {
        ElementCommandSchema {
            command: CommandId::new(id).unwrap(),
            name: name.into(),
            arguments: ElementValueKind::Null,
        }
    }

    fn element() -> ElementRegistration {
        ElementRegistration {
            element_type: ElementTypeId::new(1).unwrap(),
            name: "whisker.ui/Text".into(),
            child_policy: ChildPolicy::PlainText,
            measurement: ElementMeasurement::Text,
            text_style: false,
            properties: Vec::new(),
            events: Vec::new(),
            commands: Vec::new(),
        }
    }

    #[test]
    fn schema_binding_assigns_only_the_registry_id() {
        let registration = element();
        let schema = registration.schema();
        assert_eq!(schema.validate(), Ok(()));
        assert_eq!(
            schema.bind(ElementTypeId::new(42).unwrap()),
            ElementRegistration {
                element_type: ElementTypeId::new(42).unwrap(),
                ..registration
            }
        );
    }

    #[test]
    fn child_policy_is_independent_of_host_mounting() {
        let mut registration = element();
        registration.child_policy = ChildPolicy::Elements;
        assert_eq!(registration.validate(), Ok(()));
        assert!(registration.child_policy.accepts_elements());
        assert!(!registration.child_policy.accepts_plain_text());
        assert!(!ChildPolicy::None.accepts_elements());
        assert!(!ChildPolicy::None.accepts_plain_text());
        assert!(!ChildPolicy::PlainText.accepts_elements());
        assert!(ChildPolicy::PlainText.accepts_plain_text());
    }

    #[test]
    fn event_detail_constraint_is_optional_but_errors_are_never_data() {
        let mut event = ElementEventSchema {
            event: EventId::new(1).unwrap(),
            name: "change".into(),
            detail: None,
        };
        assert!(event.accepts_detail(&WhiskerValue::String("ready".into())));
        assert!(!event.accepts_detail(&WhiskerValue::Error("failed".into())));

        event.detail = Some(ElementValueKind::Map);
        assert!(event.accepts_detail(&WhiskerValue::map([("checked", WhiskerValue::Bool(true),)])));
        assert!(!event.accepts_detail(&WhiskerValue::Bool(true)));
    }

    #[test]
    fn value_kinds_accept_only_matching_transferable_data() {
        let cases = [
            (ElementValueKind::Null, WhiskerValue::Null),
            (ElementValueKind::Bool, WhiskerValue::Bool(true)),
            (ElementValueKind::Int, WhiskerValue::Int(1)),
            (ElementValueKind::Float, WhiskerValue::Float(1.0)),
            (
                ElementValueKind::String,
                WhiskerValue::String("value".into()),
            ),
            (ElementValueKind::Bytes, WhiskerValue::Bytes(vec![1])),
            (
                ElementValueKind::Array,
                WhiskerValue::Array(vec![WhiskerValue::Null]),
            ),
            (
                ElementValueKind::Map,
                WhiskerValue::map([("value", WhiskerValue::Null)]),
            ),
        ];
        for (kind, value) in cases {
            assert!(kind.accepts(&value));
        }
        assert!(!ElementValueKind::Null.accepts(&WhiskerValue::Bool(false)));
        assert!(!ElementValueKind::Null.accepts(&WhiskerValue::Error("failed".into())));
    }

    #[test]
    fn event_masks_cover_the_complete_wire_range() {
        assert_eq!(event(1, "first").mask(), Some(1));
        assert_eq!(event(64, "last").mask(), Some(1_u64 << 63));
        assert_eq!(event(65, "outside").mask(), None);
    }

    #[test]
    fn registration_resolves_members_by_name_and_id() {
        let mut registration = element();
        registration.properties = vec![property(1, "disabled"), property(2, "checked")];
        registration.events = vec![event(1, "focus"), event(2, "change")];
        registration.commands = vec![command(1, "focus"), command(2, "blur")];

        assert_eq!(
            registration.property_named("checked"),
            Some(&registration.properties[1])
        );
        assert_eq!(
            registration.property(PropertyId::new(2).unwrap()),
            Some(&registration.properties[1])
        );
        assert_eq!(registration.property_named("missing"), None);
        assert_eq!(registration.property(PropertyId::new(3).unwrap()), None);

        assert_eq!(
            registration.event_named("change"),
            Some(&registration.events[1])
        );
        assert_eq!(
            registration.event(EventId::new(2).unwrap()),
            Some(&registration.events[1])
        );
        assert_eq!(registration.event_named("missing"), None);
        assert_eq!(registration.event(EventId::new(3).unwrap()), None);

        assert_eq!(
            registration.command_named("blur"),
            Some(&registration.commands[1])
        );
        assert_eq!(
            registration.command(CommandId::new(2).unwrap()),
            Some(&registration.commands[1])
        );
        assert_eq!(registration.command_named("missing"), None);
        assert_eq!(registration.command(CommandId::new(3).unwrap()), None);
    }

    #[test]
    fn schema_validation_reports_every_contract_error() {
        let mut schema = element().schema();
        schema.name = " ".into();
        assert_eq!(schema.validate(), Err(ElementRegistrationError::EmptyName));

        let mut registration = element();
        registration.name.clear();
        assert_eq!(
            registration.validate(),
            Err(ElementRegistrationError::EmptyName)
        );

        let mut schema = element().schema();
        schema.properties = vec![property(1, " ")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::EmptyMemberName)
        );

        schema.properties = vec![property(1, "first"), property(1, "second")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicatePropertyId)
        );

        schema.properties = vec![property(1, "same"), property(2, "same")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicatePropertyName)
        );

        schema.properties.clear();
        schema.events = vec![event(1, " ")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::EmptyMemberName)
        );

        schema.events = vec![event(65, "outside")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::EventIdOutsideMask)
        );

        schema.events = vec![event(1, "first"), event(1, "second")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicateEventId)
        );

        schema.events = vec![event(1, "same"), event(2, "same")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicateEventName)
        );

        schema.events.clear();
        schema.commands = vec![command(1, " ")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::EmptyMemberName)
        );

        schema.commands = vec![command(1, "first"), command(1, "second")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicateCommandId)
        );

        schema.commands = vec![command(1, "same"), command(2, "same")];
        assert_eq!(
            schema.validate(),
            Err(ElementRegistrationError::DuplicateCommandName)
        );

        schema.properties = vec![property(1, "checked")];
        schema.events = vec![event(1, "change")];
        schema.commands = vec![command(1, "focus")];
        assert_eq!(schema.validate(), Ok(()));
    }
}
