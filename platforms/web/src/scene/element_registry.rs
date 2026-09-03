use std::collections::HashMap;

use whisker_protocol::{ElementRegistration, ElementTypeId};

use crate::{WebElementFactory, WebElementFactoryKind, WebError};

#[derive(Clone, Debug)]
pub(crate) struct DomElementRegistry {
    bindings: HashMap<ElementTypeId, DomElementBinding>,
}

#[derive(Clone, Debug)]
pub(crate) struct DomElementBinding {
    pub(crate) registration: ElementRegistration,
    pub(crate) factory: WebElementFactoryKind,
    pub(crate) text_content: bool,
    pub(crate) scroll_content: bool,
}

impl DomElementRegistry {
    pub(crate) fn bind(
        registrations: &[ElementRegistration],
        factories: &[WebElementFactory],
    ) -> Result<Self, WebError> {
        let mut bindings = HashMap::with_capacity(registrations.len());
        let mut elements_by_name = HashMap::with_capacity(registrations.len());
        let mut factories_by_name = HashMap::with_capacity(factories.len());
        for factory in factories {
            if matches!(&factory.kind, WebElementFactoryKind::Tag(tag) if tag.trim().is_empty()) {
                return Err(WebError(format!(
                    "DOM factory {} has an empty tag name",
                    factory.name
                )));
            }
            if factories_by_name
                .insert(factory.name.clone(), factory.clone())
                .is_some()
            {
                return Err(WebError(format!("duplicate DOM factory {}", factory.name)));
            }
        }
        for registration in registrations {
            registration.validate().map_err(|error| {
                WebError(format!(
                    "invalid DOM element {}: {error:?}",
                    registration.name
                ))
            })?;
            if bindings.contains_key(&registration.element_type) {
                return Err(WebError(format!(
                    "duplicate DOM element type {}",
                    registration.element_type.get()
                )));
            }
            if elements_by_name
                .insert(registration.name.clone(), registration.element_type)
                .is_some()
            {
                return Err(WebError(format!(
                    "duplicate DOM element {}",
                    registration.name
                )));
            }
            let factory = factories_by_name
                .remove(&registration.name)
                .ok_or_else(|| WebError(format!("missing DOM factory {}", registration.name)))?
                .bind(registration)?;
            bindings.insert(
                registration.element_type,
                DomElementBinding {
                    registration: registration.clone(),
                    factory: factory.kind,
                    text_content: factory.text_content,
                    scroll_content: factory.scroll_content,
                },
            );
        }
        if let Some(name) = factories_by_name.into_keys().next() {
            return Err(WebError(format!(
                "DOM factory {name} has no Rust element schema"
            )));
        }
        Ok(Self { bindings })
    }

    pub(crate) fn binding(
        &self,
        element_type: ElementTypeId,
    ) -> Result<&DomElementBinding, WebError> {
        self.bindings.get(&element_type).ok_or_else(|| {
            WebError(format!(
                "DOM Host received unknown element type {}",
                element_type.get()
            ))
        })
    }
}
