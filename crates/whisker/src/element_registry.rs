//! Bootstrap-time normalization of built-in and module-provided elements.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::ElementTag;
use whisker_engine::whisker_protocol::{
    ElementRegistration, ElementRegistrationError, ElementSchema, ElementTypeId,
};
use whisker_engine::whisker_style::SpecifiedStyle;

type SchemaKey = String;

/// Authoring syntax generated for one element provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementAuthoringBinding {
    /// A standard `render!` tag implemented through the provider path.
    Builtin(ElementTag),
    /// A generated module component resolved by its schema name.
    Named,
}

/// Generated bootstrap metadata for one UI-providing module definition.
///
/// Service-only modules do not emit this value and continue through the
/// existing function, event, and observer registration path. This is an owned
/// description rather than a public factory trait so build-generated metadata
/// can be normalized before a target Host binds its implementation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementProviderMetadata {
    /// Host-independent element contract.
    pub schema: ElementSchema,
    /// Authoring syntax that resolves to this contract.
    pub authoring: ElementAuthoringBinding,
    /// Host-independent declarations applied before the caller's style.
    pub base_style: SpecifiedStyle,
}

/// Generated Rust-side definition for one element-provider module.
///
/// A module owns its element schemas and authoring bindings as one bootstrap
/// unit. Host packages bind target-specific factories to the resulting
/// element names before the first frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementModuleDefinition {
    /// Stable package/module identity used in bootstrap diagnostics.
    pub module_name: String,
    /// Visual elements exported by this module.
    pub elements: Vec<ElementProviderMetadata>,
}

impl ElementModuleDefinition {
    /// Creates an element-provider module definition.
    pub fn new(
        module_name: impl Into<String>,
        elements: impl IntoIterator<Item = ElementProviderMetadata>,
    ) -> Self {
        Self {
            module_name: module_name.into(),
            elements: elements.into_iter().collect(),
        }
    }
}

impl ElementProviderMetadata {
    /// Describes one standard element implemented through the provider path.
    pub fn builtin(tag: ElementTag, schema: ElementSchema) -> Self {
        Self {
            schema,
            authoring: ElementAuthoringBinding::Builtin(tag),
            base_style: SpecifiedStyle::new(),
        }
    }

    /// Describes one generated module element.
    ///
    /// Its Rust authoring name, schema name, and Host binding key are the same
    /// package-qualified [`ElementSchema::name`].
    pub fn named(schema: ElementSchema) -> Self {
        Self {
            schema,
            authoring: ElementAuthoringBinding::Named,
            base_style: SpecifiedStyle::new(),
        }
    }

    /// Applies provider-owned defaults while preserving caller declarations as
    /// the later, overriding style fragment.
    pub fn with_base_style(mut self, style: SpecifiedStyle) -> Self {
        self.base_style = style;
        self
    }
}

/// Immutable element contracts and authoring bindings for one registry epoch.
///
/// Compact IDs are assigned when an [`ElementRegistryBuilder`] is built. They
/// are therefore independent of [`ElementTag`] discriminants and are never
/// supplied by a module schema.
#[derive(Clone, Debug)]
pub struct ElementRegistry {
    registrations: Vec<ElementRegistration>,
    base_styles: Vec<SpecifiedStyle>,
    builtins: HashMap<ElementTag, usize>,
    names: HashMap<String, usize>,
}

impl ElementRegistry {
    /// Starts an empty registry definition.
    pub fn builder() -> ElementRegistryBuilder {
        ElementRegistryBuilder::default()
    }

    /// Starts a registry definition containing the standard Whisker elements.
    ///
    /// Callers can append module schemas and authoring-name bindings before
    /// building the immutable surface registry.
    pub fn standard_builder() -> ElementRegistryBuilder {
        Self::builder().register_module(crate::standard_ui::standard_ui_module_definition())
    }

    /// Returns the standard Whisker element registry.
    pub fn standard() -> Self {
        Self::standard_builder()
            .build()
            .expect("standard element schemas are valid and uniquely bound")
    }

    /// Returns the normalized contracts in compact-ID order.
    pub fn registrations(&self) -> &[ElementRegistration] {
        &self.registrations
    }

    pub(crate) fn base_style(&self, registration: &ElementRegistration) -> &SpecifiedStyle {
        let index = registration.element_type.get() as usize - 1;
        self.base_styles
            .get(index)
            .expect("registrations and base styles share one compact-ID order")
    }

    /// Resolves one built-in authoring tag to its normalized contract.
    pub fn registration_for_builtin(&self, tag: ElementTag) -> Option<&ElementRegistration> {
        self.builtins
            .get(&tag)
            .and_then(|index| self.registrations.get(*index))
    }

    /// Resolves one generated or compatibility authoring name.
    pub fn registration_for_name(&self, name: &str) -> Option<&ElementRegistration> {
        self.names
            .get(name)
            .and_then(|index| self.registrations.get(*index))
    }

    /// Ensures a named module schema belongs to this registry epoch and
    /// returns its compact registration. Repeating the same declaration is
    /// idempotent; changing a schema under an existing name is rejected.
    pub fn register_named(
        &mut self,
        schema: ElementSchema,
    ) -> Result<&ElementRegistration, ElementRegistryError> {
        schema
            .validate()
            .map_err(|error| ElementRegistryError::InvalidSchema {
                name: schema.name.clone(),
                error,
            })?;
        if let Some(index) = self.names.get(&schema.name).copied() {
            let registration = &self.registrations[index];
            if registration.schema() != schema {
                return Err(ElementRegistryError::ConflictingSchema {
                    name: registration.name.clone(),
                });
            }
            return Ok(registration);
        }
        let raw_id = u32::try_from(self.registrations.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .and_then(ElementTypeId::new)
            .ok_or(ElementRegistryError::ElementTypeIdExhausted)?;
        let name = schema.name.clone();
        let index = self.registrations.len();
        self.registrations.push(schema.bind(raw_id));
        self.base_styles.push(SpecifiedStyle::new());
        self.names.insert(name, index);
        Ok(&self.registrations[index])
    }
}

/// Mutable bootstrap description used to construct an [`ElementRegistry`].
#[derive(Clone, Debug, Default)]
pub struct ElementRegistryBuilder {
    schemas: Vec<(ElementSchema, SpecifiedStyle)>,
    builtin_bindings: Vec<(ElementTag, SchemaKey)>,
    name_bindings: Vec<(String, SchemaKey)>,
}

impl ElementRegistryBuilder {
    /// Adds every generated element exported by one module definition.
    pub fn register_module(self, definition: ElementModuleDefinition) -> Self {
        self.register_providers(definition.elements)
    }

    /// Adds a sequence of generated element-provider modules.
    pub fn register_modules(
        mut self,
        definitions: impl IntoIterator<Item = ElementModuleDefinition>,
    ) -> Self {
        for definition in definitions {
            self = self.register_module(definition);
        }
        self
    }

    /// Adds generated metadata for one UI-providing module.
    pub fn register_provider(mut self, provider: ElementProviderMetadata) -> Self {
        let name = provider.schema.name.clone();
        self.schemas.push((provider.schema, provider.base_style));
        match provider.authoring {
            ElementAuthoringBinding::Builtin(tag) => {
                self.builtin_bindings.push((tag, name));
            }
            ElementAuthoringBinding::Named => {
                self.name_bindings.push((name.clone(), name));
            }
        }
        self
    }

    /// Adds every UI provider emitted by generated application metadata.
    pub fn register_providers(
        mut self,
        providers: impl IntoIterator<Item = ElementProviderMetadata>,
    ) -> Self {
        for provider in providers {
            self = self.register_provider(provider);
        }
        self
    }

    /// Adds a Host-independent schema to the registry epoch.
    ///
    /// Prefer [`Self::register_provider`] for generated module metadata. This
    /// lower-level method supports schemas whose authoring binding is supplied
    /// separately or which are retained only for Host negotiation.
    pub fn register(mut self, schema: ElementSchema) -> Self {
        self.schemas.push((schema, SpecifiedStyle::new()));
        self
    }

    /// Maps a built-in authoring tag to a schema name.
    pub fn bind_builtin(mut self, tag: ElementTag, name: impl Into<String>) -> Self {
        self.builtin_bindings.push((tag, name.into()));
        self
    }

    /// Maps a generated or compatibility authoring name to a schema key.
    pub fn bind_name(
        mut self,
        authoring_name: impl Into<String>,
        element_name: impl Into<String>,
    ) -> Self {
        self.name_bindings
            .push((authoring_name.into(), element_name.into()));
        self
    }

    /// Validates schemas and bindings, then assigns compact IDs for the epoch.
    pub fn build(self) -> Result<ElementRegistry, ElementRegistryError> {
        let mut registrations = Vec::with_capacity(self.schemas.len());
        let mut base_styles = Vec::with_capacity(self.schemas.len());
        let mut schemas = HashMap::with_capacity(self.schemas.len());
        for (index, (schema, base_style)) in self.schemas.into_iter().enumerate() {
            schema
                .validate()
                .map_err(|error| ElementRegistryError::InvalidSchema {
                    name: schema.name.clone(),
                    error,
                })?;
            let key = schema.name.clone();
            if schemas.contains_key(&key) {
                return Err(ElementRegistryError::DuplicateName { name: key });
            }
            let raw_id = u32::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .and_then(ElementTypeId::new)
                .ok_or(ElementRegistryError::ElementTypeIdExhausted)?;
            schemas.insert(key, registrations.len());
            registrations.push(schema.bind(raw_id));
            base_styles.push(base_style);
        }

        let mut builtins = HashMap::with_capacity(self.builtin_bindings.len());
        for (tag, key) in self.builtin_bindings {
            if tag == ElementTag::RawText {
                return Err(ElementRegistryError::VirtualBuiltinBinding { tag });
            }
            let index = schemas
                .get(&key)
                .copied()
                .ok_or_else(|| ElementRegistryError::UnknownSchemaBinding { name: key.clone() })?;
            if builtins.insert(tag, index).is_some() {
                return Err(ElementRegistryError::DuplicateBuiltinBinding { tag });
            }
        }

        let mut names = HashMap::with_capacity(self.name_bindings.len());
        for (name, key) in self.name_bindings {
            if name.trim().is_empty() {
                return Err(ElementRegistryError::EmptyAuthoringName);
            }
            let index = schemas
                .get(&key)
                .copied()
                .ok_or_else(|| ElementRegistryError::UnknownSchemaBinding { name: key.clone() })?;
            if names.insert(name.clone(), index).is_some() {
                return Err(ElementRegistryError::DuplicateAuthoringName { name });
            }
        }

        Ok(ElementRegistry {
            registrations,
            base_styles,
            builtins,
            names,
        })
    }
}

/// Bootstrap failure while normalizing an element registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementRegistryError {
    /// A schema violated a Host-independent contract invariant.
    InvalidSchema {
        /// Element name retained for diagnostics.
        name: String,
        /// Rejected contract combination.
        error: ElementRegistrationError,
    },
    /// Two schemas declared the same name.
    DuplicateName {
        /// Conflicting package-qualified name.
        name: String,
    },
    /// A declaration reused a name with a different contract.
    ConflictingSchema {
        /// Stable name whose contract changed within one registry epoch.
        name: String,
    },
    /// A binding referred to a schema absent from this registry definition.
    UnknownSchemaBinding {
        /// Missing package-qualified name.
        name: String,
    },
    /// A built-in tag was bound more than once.
    DuplicateBuiltinBinding {
        /// Conflicting authoring tag.
        tag: ElementTag,
    },
    /// Virtual raw text cannot have a Host element schema.
    VirtualBuiltinBinding {
        /// Rejected virtual tag.
        tag: ElementTag,
    },
    /// A generated or compatibility authoring name was empty.
    EmptyAuthoringName,
    /// A generated or compatibility authoring name was bound more than once.
    DuplicateAuthoringName {
        /// Conflicting authoring name.
        name: String,
    },
    /// The registry cannot allocate another non-zero compact element ID.
    ElementTypeIdExhausted,
}

impl fmt::Display for ElementRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker element registry error: {self:?}")
    }
}

impl Error for ElementRegistryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_engine::whisker_protocol::{ChildPolicy, ElementMeasurement};

    fn custom_schema(name: &str) -> ElementSchema {
        ElementSchema {
            name: name.into(),
            child_policy: ChildPolicy::Elements,
            measurement: ElementMeasurement::None,
            properties: Vec::new(),
            events: Vec::new(),
            commands: Vec::new(),
        }
    }

    #[test]
    fn standard_and_module_schemas_share_one_id_allocator() {
        let registry = ElementRegistry::standard_builder()
            .register_module(ElementModuleDefinition::new(
                "example.maps",
                [ElementProviderMetadata::named(custom_schema(
                    "example.maps/Map",
                ))],
            ))
            .build()
            .unwrap();

        assert_eq!(registry.registrations().len(), 4);
        assert_eq!(
            registry
                .registration_for_name("example.maps/Map")
                .unwrap()
                .element_type,
            ElementTypeId::new(4).unwrap()
        );
        assert!(
            registry
                .registration_for_builtin(ElementTag::Page)
                .is_none()
        );
        assert_ne!(
            registry
                .registration_for_builtin(ElementTag::ScrollView)
                .unwrap()
                .element_type
                .get(),
            ElementTag::ScrollView as u32
        );
    }

    #[test]
    fn duplicate_names_fail_before_a_surface_starts() {
        let result = ElementRegistry::builder()
            .register(custom_schema("example/Map"))
            .register(custom_schema("example/Map"))
            .build();

        assert!(matches!(
            result,
            Err(ElementRegistryError::DuplicateName { .. })
        ));
    }

    #[test]
    fn authoring_bindings_must_resolve_uniquely() {
        let result = ElementRegistry::builder()
            .register(custom_schema("example/Map"))
            .bind_name("map", "example/Map")
            .bind_name("map", "example/Map")
            .build();
        assert_eq!(
            result.unwrap_err(),
            ElementRegistryError::DuplicateAuthoringName { name: "map".into() }
        );
    }
}
