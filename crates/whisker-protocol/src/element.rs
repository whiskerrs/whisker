//! Element schemas negotiated before a surface presents its first frame.

use crate::ElementTypeId;

/// Host-independent element contract before a surface assigns compact IDs.
///
/// Generated module metadata and built-in primitives both enter bootstrap in
/// this form. The resulting surface registry turns each schema into an
/// [`ElementRegistration`] with an immutable [`ElementTypeId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementSchema {
    /// Stable, versionless package-qualified name.
    pub canonical_name: String,
    /// Element-specific content channel.
    pub content: ElementContentKind,
    /// Host mount target for ordinary scene children.
    pub child_mount: ElementChildMount,
    /// Intrinsic measurement behavior.
    pub measurement: ElementMeasurement,
    /// Whether the Host content receives resolved inherited text style.
    pub consumes_text_style: bool,
}

impl ElementSchema {
    /// Validates schema invariants independent of a particular Host backend.
    pub fn validate(&self) -> Result<(), ElementRegistrationError> {
        validate_contract(
            &self.canonical_name,
            self.content,
            self.child_mount,
            self.measurement,
            self.consumes_text_style,
        )
    }

    /// Binds this schema to a compact ID allocated for one registry epoch.
    pub fn bind(self, element_type: ElementTypeId) -> ElementRegistration {
        ElementRegistration {
            element_type,
            canonical_name: self.canonical_name,
            content: self.content,
            child_mount: self.child_mount,
            measurement: self.measurement,
            consumes_text_style: self.consumes_text_style,
        }
    }
}

/// Host content owned by one element type.
///
/// Common box presentation is deliberately absent from this enum. Layout,
/// background, borders, clipping, opacity, transforms, and stacking are
/// handled by the Host renderer for every registered element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementContentKind {
    /// The element contributes only common box presentation and children.
    None,
    /// Plain shaped text supplied by [`Operation::SetText`](crate::Operation::SetText).
    Text,
    /// Replaced image content.
    Image,
    /// A platform-native editable text control.
    EditableText,
    /// Other platform-native or external-surface content.
    Native,
    /// A platform scroll container with a separate content mount target.
    ScrollContainer,
}

/// Where ordinary Whisker children are mounted in the Host projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementChildMount {
    /// The element is a leaf and rejects ordinary scene children.
    None,
    /// Children mount in the common presentation container.
    Presentation,
    /// Children mount in the content container owned by a scroll element.
    ScrollContent,
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
    /// An element-specific Host measurer supplies intrinsic metrics.
    Host,
}

/// One element contract bound to a compact type ID for a surface registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementRegistration {
    /// Compact identifier carried by frame and measurement packets.
    pub element_type: ElementTypeId,
    /// Stable, versionless package-qualified name.
    pub canonical_name: String,
    /// Element-specific content channel.
    pub content: ElementContentKind,
    /// Host mount target for ordinary scene children.
    pub child_mount: ElementChildMount,
    /// Intrinsic measurement behavior.
    pub measurement: ElementMeasurement,
    /// Whether the Host content receives resolved inherited text style.
    pub consumes_text_style: bool,
}

impl ElementRegistration {
    /// Validates schema invariants independent of a particular Host backend.
    pub fn validate(&self) -> Result<(), ElementRegistrationError> {
        validate_contract(
            &self.canonical_name,
            self.content,
            self.child_mount,
            self.measurement,
            self.consumes_text_style,
        )
    }

    /// Returns the Host-independent schema represented by this registration.
    pub fn schema(&self) -> ElementSchema {
        ElementSchema {
            canonical_name: self.canonical_name.clone(),
            content: self.content,
            child_mount: self.child_mount,
            measurement: self.measurement,
            consumes_text_style: self.consumes_text_style,
        }
    }
}

fn validate_contract(
    canonical_name: &str,
    content: ElementContentKind,
    child_mount: ElementChildMount,
    measurement: ElementMeasurement,
    consumes_text_style: bool,
) -> Result<(), ElementRegistrationError> {
    if canonical_name.trim().is_empty() {
        return Err(ElementRegistrationError::EmptyCanonicalName);
    }
    if content == ElementContentKind::Text {
        if child_mount != ElementChildMount::None {
            return Err(ElementRegistrationError::TextMustBeLeaf);
        }
        if measurement != ElementMeasurement::Text {
            return Err(ElementRegistrationError::TextMeasurementMismatch);
        }
        if !consumes_text_style {
            return Err(ElementRegistrationError::TextStyleRequired);
        }
    }
    if child_mount == ElementChildMount::ScrollContent
        && content != ElementContentKind::ScrollContainer
    {
        return Err(ElementRegistrationError::ScrollMountWithoutContainer);
    }
    if content == ElementContentKind::ScrollContainer
        && child_mount != ElementChildMount::ScrollContent
    {
        return Err(ElementRegistrationError::ScrollContainerWithoutMount);
    }
    Ok(())
}

/// Invalid Host-independent element registration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementRegistrationError {
    /// The package-qualified element name was empty.
    EmptyCanonicalName,
    /// Text content cannot contain ordinary scene children.
    TextMustBeLeaf,
    /// Text content did not select common text measurement.
    TextMeasurementMismatch,
    /// Text content did not request the resolved text-style channel.
    TextStyleRequired,
    /// A scroll-content mount target was declared by non-scroll content.
    ScrollMountWithoutContainer,
    /// Scroll content did not provide its required child mount target.
    ScrollContainerWithoutMount,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> ElementRegistration {
        ElementRegistration {
            element_type: ElementTypeId::new(1).unwrap(),
            canonical_name: "whisker.ui/Text".into(),
            content: ElementContentKind::Text,
            child_mount: ElementChildMount::None,
            measurement: ElementMeasurement::Text,
            consumes_text_style: true,
        }
    }

    #[test]
    fn schema_binding_assigns_only_the_registry_id() {
        let registration = text();
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
    fn valid_text_and_scroll_contracts_are_accepted() {
        assert_eq!(text().validate(), Ok(()));
        assert_eq!(
            ElementRegistration {
                element_type: ElementTypeId::new(2).unwrap(),
                canonical_name: "whisker.ui/ScrollView".into(),
                content: ElementContentKind::ScrollContainer,
                child_mount: ElementChildMount::ScrollContent,
                measurement: ElementMeasurement::None,
                consumes_text_style: false,
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn invalid_category_combinations_are_rejected() {
        let mut registration = text();
        registration.child_mount = ElementChildMount::Presentation;
        assert_eq!(
            registration.validate(),
            Err(ElementRegistrationError::TextMustBeLeaf)
        );

        let mut registration = text();
        registration.measurement = ElementMeasurement::None;
        assert_eq!(
            registration.validate(),
            Err(ElementRegistrationError::TextMeasurementMismatch)
        );

        let mut registration = text();
        registration.consumes_text_style = false;
        assert_eq!(
            registration.validate(),
            Err(ElementRegistrationError::TextStyleRequired)
        );
    }
}
