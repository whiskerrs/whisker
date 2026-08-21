//! Desktop binding for negotiated element schemas.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use whisker::ElementProviderMetadata;
use whisker_protocol::{
    ElementChildMount, ElementContentKind, ElementMeasurement, ElementRegistration,
    ElementRegistrationError, ElementTypeId, NodeId, TextContent,
};

/// One Desktop element module's Rust schema and target factory.
///
/// Keeping both halves in one value prevents application composition from
/// selecting an element schema without embedding its Desktop implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopElementModule {
    provider: ElementProviderMetadata,
    factory: DesktopElementFactory,
}

impl DesktopElementModule {
    /// Joins a Rust provider to its Desktop Host factory.
    pub fn new(provider: ElementProviderMetadata, factory: DesktopElementFactory) -> Self {
        Self { provider, factory }
    }

    /// Returns the Rust provider metadata.
    pub fn provider(&self) -> &ElementProviderMetadata {
        &self.provider
    }

    /// Returns the target-specific Desktop factory.
    pub fn factory(&self) -> &DesktopElementFactory {
        &self.factory
    }
}

/// Target-specific Desktop factory embedded for one element module.
///
/// The canonical name joins this Host definition to the Rust-side
/// [`whisker::ElementProviderMetadata`]. Constructors intentionally expose only
/// the content factories implemented by the current Desktop Host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopElementFactory {
    canonical_name: String,
    kind: DesktopElementFactoryKind,
}

impl DesktopElementFactory {
    /// Creates a common-presentation-only element factory.
    pub fn presentation(canonical_name: impl Into<String>) -> Self {
        Self::new(canonical_name, DesktopElementFactoryKind::Presentation)
    }

    /// Creates a native Desktop text-content factory.
    pub fn text(canonical_name: impl Into<String>) -> Self {
        Self::new(canonical_name, DesktopElementFactoryKind::Text)
    }

    /// Creates a Desktop scroll-container factory.
    pub fn scroll_container(canonical_name: impl Into<String>) -> Self {
        Self::new(canonical_name, DesktopElementFactoryKind::ScrollContainer)
    }

    fn new(canonical_name: impl Into<String>, kind: DesktopElementFactoryKind) -> Self {
        Self {
            canonical_name: canonical_name.into(),
            kind,
        }
    }

    fn content(&self) -> ElementContentKind {
        match self.kind {
            DesktopElementFactoryKind::Presentation => ElementContentKind::None,
            DesktopElementFactoryKind::Text => ElementContentKind::Text,
            DesktopElementFactoryKind::ScrollContainer => ElementContentKind::ScrollContainer,
        }
    }

    fn create(&self) -> DesktopElementContent {
        match self.kind {
            DesktopElementFactoryKind::Presentation => DesktopElementContent::Empty,
            DesktopElementFactoryKind::Text => DesktopElementContent::Text(None),
            DesktopElementFactoryKind::ScrollContainer => DesktopElementContent::ScrollContainer,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesktopElementFactoryKind {
    Presentation,
    Text,
    ScrollContainer,
}

/// Returns the standard UI package as ordinary Desktop element modules.
pub fn standard_desktop_element_modules() -> Vec<DesktopElementModule> {
    whisker::standard_element_providers()
        .into_iter()
        .map(|provider| {
            let factory = match provider.schema.canonical_name.as_str() {
                whisker::VIEW_ELEMENT_NAME => {
                    DesktopElementFactory::presentation(whisker::VIEW_ELEMENT_NAME)
                }
                whisker::TEXT_ELEMENT_NAME => {
                    DesktopElementFactory::text(whisker::TEXT_ELEMENT_NAME)
                }
                whisker::SCROLL_VIEW_ELEMENT_NAME => {
                    DesktopElementFactory::scroll_container(whisker::SCROLL_VIEW_ELEMENT_NAME)
                }
                canonical_name => panic!("standard UI factory missing for {canonical_name}"),
            };
            DesktopElementModule::new(provider, factory)
        })
        .collect()
}

/// Returns only the Desktop factories from the standard UI package.
pub fn standard_desktop_element_factories() -> Vec<DesktopElementFactory> {
    standard_desktop_element_modules()
        .into_iter()
        .map(|module| module.factory)
        .collect()
}

/// Element-specific state retained beside common Desktop presentation.
#[derive(Clone, Debug)]
pub(crate) enum DesktopElementContent {
    Empty,
    Text(Option<TextContent>),
    ScrollContainer,
}

impl DesktopElementContent {
    pub(crate) fn text(&self) -> Option<&TextContent> {
        match self {
            Self::Text(content) => content.as_ref(),
            Self::Empty | Self::ScrollContainer => None,
        }
    }

    pub(crate) fn set_text(
        &mut self,
        node: NodeId,
        content: TextContent,
    ) -> Result<(), DesktopElementError> {
        match self {
            Self::Text(current) => {
                *current = Some(content);
                Ok(())
            }
            Self::Empty | Self::ScrollContainer => {
                Err(DesktopElementError::UnexpectedText { node })
            }
        }
    }
}

#[derive(Clone, Debug)]
struct DesktopElementBinding {
    factory: DesktopElementFactory,
    child_mount: ElementChildMount,
    measurement: ElementMeasurement,
}

/// Immutable element factories bound before the first Desktop frame.
///
/// Common presentation never dispatches through this registry. It is consulted
/// only when a node is created or receives an element-specific content
/// operation, keeping style and layout updates on the existing dense path.
#[derive(Clone, Debug)]
pub(crate) struct DesktopElementRegistry {
    bindings: HashMap<ElementTypeId, DesktopElementBinding>,
}

impl DesktopElementRegistry {
    pub(crate) fn bind(
        registrations: &[ElementRegistration],
        factories: &[DesktopElementFactory],
    ) -> Result<Self, DesktopElementError> {
        let mut bindings = HashMap::with_capacity(registrations.len());
        let mut canonical = HashMap::with_capacity(registrations.len());
        let mut factories_by_name = HashMap::with_capacity(factories.len());
        for factory in factories {
            if factories_by_name
                .insert(factory.canonical_name.clone(), factory.clone())
                .is_some()
            {
                return Err(DesktopElementError::DuplicateFactory {
                    canonical_name: factory.canonical_name.clone(),
                });
            }
        }
        for registration in registrations {
            registration
                .validate()
                .map_err(|error| DesktopElementError::InvalidRegistration {
                    element_type: registration.element_type,
                    error,
                })?;
            if bindings.contains_key(&registration.element_type) {
                return Err(DesktopElementError::DuplicateElementType {
                    element_type: registration.element_type,
                });
            }
            let identity = registration.canonical_name.clone();
            if canonical
                .insert(identity, registration.element_type)
                .is_some()
            {
                return Err(DesktopElementError::DuplicateCanonicalElement {
                    element_type: registration.element_type,
                });
            }
            let factory = factories_by_name
                .remove(&registration.canonical_name)
                .ok_or_else(|| DesktopElementError::MissingFactory {
                    canonical_name: registration.canonical_name.clone(),
                })?;
            if registration.content != factory.content() {
                return Err(DesktopElementError::FactoryMismatch {
                    canonical_name: registration.canonical_name.clone(),
                    schema_content: registration.content,
                    factory_content: factory.content(),
                });
            }
            bindings.insert(
                registration.element_type,
                DesktopElementBinding {
                    factory,
                    child_mount: registration.child_mount,
                    measurement: registration.measurement,
                },
            );
        }
        if let Some(canonical_name) = factories_by_name.into_keys().next() {
            return Err(DesktopElementError::UnknownFactory { canonical_name });
        }
        Ok(Self { bindings })
    }

    pub(crate) fn create(
        &self,
        element_type: ElementTypeId,
    ) -> Result<DesktopElementContent, DesktopElementError> {
        Ok(self.binding(element_type)?.factory.create())
    }

    pub(crate) fn child_mount(
        &self,
        element_type: ElementTypeId,
    ) -> Result<ElementChildMount, DesktopElementError> {
        Ok(self.binding(element_type)?.child_mount)
    }

    pub(crate) fn measurement(
        &self,
        element_type: ElementTypeId,
    ) -> Result<ElementMeasurement, DesktopElementError> {
        Ok(self.binding(element_type)?.measurement)
    }

    fn binding(
        &self,
        element_type: ElementTypeId,
    ) -> Result<DesktopElementBinding, DesktopElementError> {
        self.bindings
            .get(&element_type)
            .cloned()
            .ok_or(DesktopElementError::UnknownElementType { element_type })
    }
}

/// Desktop element registration or content-dispatch failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DesktopElementError {
    InvalidRegistration {
        element_type: ElementTypeId,
        error: ElementRegistrationError,
    },
    DuplicateElementType {
        element_type: ElementTypeId,
    },
    DuplicateCanonicalElement {
        element_type: ElementTypeId,
    },
    DuplicateFactory {
        canonical_name: String,
    },
    MissingFactory {
        canonical_name: String,
    },
    UnknownFactory {
        canonical_name: String,
    },
    FactoryMismatch {
        canonical_name: String,
        schema_content: ElementContentKind,
        factory_content: ElementContentKind,
    },
    UnknownElementType {
        element_type: ElementTypeId,
    },
    ChildrenNotAllowed {
        parent: NodeId,
    },
    UnexpectedText {
        node: NodeId,
    },
    UnsupportedProperty {
        node: NodeId,
    },
    UnsupportedCommand {
        node: NodeId,
    },
}

impl fmt::Display for DesktopElementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Desktop element error: {self:?}")
    }
}

impl Error for DesktopElementError {}

#[cfg(test)]
mod tests {
    use whisker::{
        ElementProviderMetadata, ElementRegistry, SurfaceRuntime, standard_element_registrations,
    };
    use whisker_protocol::{ElementMeasurement, ElementSchema, SurfaceId};
    use whisker_style::StyleEnvironment;

    use super::*;

    #[test]
    fn standard_view_text_and_scroll_bind_through_one_registry() {
        let registrations = standard_element_registrations();
        let factories = standard_desktop_element_factories();
        let registry = DesktopElementRegistry::bind(&registrations, &factories).unwrap();
        for registration in &registrations {
            let content = registry.create(registration.element_type).unwrap();
            assert_eq!(
                matches!(content, DesktopElementContent::Text(_)),
                registration.content == ElementContentKind::Text
            );
            assert_eq!(
                registry.child_mount(registration.element_type).unwrap(),
                registration.child_mount
            );
            assert_eq!(
                registry.measurement(registration.element_type).unwrap(),
                registration.measurement
            );
        }

        let surface = SurfaceRuntime::new(
            SurfaceId::new(1).unwrap(),
            StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
        );
        assert_eq!(surface.element_registrations(), registrations);
        assert!(registrations.iter().any(|registration| {
            registration.content == ElementContentKind::Text
                && registration.measurement == ElementMeasurement::Text
        }));
    }

    #[test]
    fn duplicate_and_unsupported_registrations_fail_before_a_frame() {
        let mut registrations = standard_element_registrations();
        registrations.push(registrations[0].clone());
        assert!(matches!(
            DesktopElementRegistry::bind(&registrations, &standard_desktop_element_factories()),
            Err(DesktopElementError::DuplicateElementType { .. })
        ));

        let mut unsupported = standard_element_registrations()[0].clone();
        unsupported.content = ElementContentKind::Native;
        assert!(matches!(
            DesktopElementRegistry::bind(
                &[unsupported],
                &[DesktopElementFactory::presentation("whisker.ui/View")]
            ),
            Err(DesktopElementError::FactoryMismatch { .. })
        ));

        assert!(matches!(
            DesktopElementRegistry::bind(&standard_element_registrations(), &[]),
            Err(DesktopElementError::MissingFactory { .. })
        ));
    }

    #[test]
    fn module_provider_binds_by_versionless_canonical_name() {
        let module = DesktopElementModule::new(
            ElementProviderMetadata::named(
                "badge",
                ElementSchema {
                    canonical_name: "whisker.test/Badge".into(),
                    content: ElementContentKind::None,
                    child_mount: ElementChildMount::Presentation,
                    measurement: ElementMeasurement::None,
                    consumes_text_style: false,
                },
            ),
            DesktopElementFactory::presentation("whisker.test/Badge"),
        );
        let elements = ElementRegistry::standard_builder()
            .register_provider(module.provider().clone())
            .build()
            .unwrap();
        let badge = elements.registration_for_name("badge").unwrap();
        let mut factories = standard_desktop_element_factories();
        factories.push(module.factory().clone());
        let desktop = DesktopElementRegistry::bind(elements.registrations(), &factories).unwrap();

        assert!(matches!(
            desktop.create(badge.element_type),
            Ok(DesktopElementContent::Empty)
        ));
        assert_eq!(badge.canonical_name, "whisker.test/Badge");
    }
}
