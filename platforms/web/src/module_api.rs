use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use whisker::{ElementModuleDefinition, WhiskerModule};
use whisker_protocol::{CommandId, ElementRegistration, PropertyId};

use crate::{WebError, WhiskerValue};

/// Configuration for one browser surface.
#[derive(Clone, Debug)]
pub struct WebAppConfig {
    /// Document title.
    pub title: String,
    /// DOM element id used as the surface root.
    pub root_id: String,
    /// Element modules selected for this target.
    pub module_definitions: Vec<WebModuleDefinition>,
    /// Host-independent element schemas selected from Rust module crates.
    pub element_modules: Vec<ElementModuleDefinition>,
}

impl WebAppConfig {
    /// Creates a browser configuration rooted at `#whisker-root`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            root_id: "whisker-root".to_string(),
            module_definitions: Vec::new(),
            element_modules: Vec::new(),
        }
    }

    /// Adds one Rust element definition with its matching DOM factory.
    pub fn with_module_definition(mut self, definition: WebModuleDefinition) -> Self {
        self.module_definitions.push(definition);
        self
    }

    /// Adds one Host-independent Rust element module for bootstrap negotiation.
    pub fn with_element_module(mut self, definition: ElementModuleDefinition) -> Self {
        self.element_modules.push(definition);
        self
    }
}

/// Rust/Web counterpart of the Swift/Kotlin `ModuleDefinition` DSL.
#[derive(Clone, Debug, Default)]
pub struct WebModuleDefinition {
    factories: Vec<WebElementFactory>,
}

/// Web Host module declaration, named consistently with native Hosts.
pub type ModuleDefinition = WebModuleDefinition;

impl WebModuleDefinition {
    /// Starts an empty Web module declaration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an independently declared Host View matched by stable name.
    pub fn view<I>(mut self, implementation: I) -> Self
    where
        I: WebViewImplementation,
    {
        self.factories.push(implementation.into_web_factory());
        self
    }

    /// Returns every DOM factory contributed by this declaration.
    pub fn factories(&self) -> &[WebElementFactory] {
        &self.factories
    }

    /// Consumes the declaration and returns its DOM factories.
    pub fn into_factories(self) -> Vec<WebElementFactory> {
        self.factories
    }
}

/// Converts one Web `View` declaration into its Host factory.
pub trait WebViewImplementation {
    /// Erases this declaration into a factory bound by name at bootstrap.
    fn into_web_factory(self) -> WebElementFactory;
}

impl WebViewImplementation for WebElementFactory {
    fn into_web_factory(self) -> WebElementFactory {
        self
    }
}

type WebViewConstructor<T> =
    Rc<dyn Fn(&web_sys::Document, WebEventEmitter) -> Result<T, wasm_bindgen::JsValue>>;
type WebElementGetter<T> = Rc<dyn Fn(&T) -> web_sys::Element>;
type WebPropSetter<T> = Rc<dyn Fn(&mut T, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue>>;
type WebPropClearer<T> = Rc<dyn Fn(&mut T) -> Result<(), wasm_bindgen::JsValue>>;
type WebCommandHandler<T> = Rc<dyn Fn(&mut T, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue>>;

struct WebPropBinding<T> {
    set: WebPropSetter<T>,
    clear: WebPropClearer<T>,
}

impl<T> Clone for WebPropBinding<T> {
    fn clone(&self) -> Self {
        Self {
            set: Rc::clone(&self.set),
            clear: Rc::clone(&self.clear),
        }
    }
}

/// The `View { Prop / Events / Command }` portion of a Web declaration.
pub struct WebViewDefinition<T> {
    name: String,
    create: WebViewConstructor<T>,
    element: WebElementGetter<T>,
    properties: HashMap<String, WebPropBinding<T>>,
    events: HashSet<String>,
    commands: HashMap<String, WebCommandHandler<T>>,
    plain_text: bool,
    scroll_content: bool,
}

impl<T> WebViewDefinition<T>
where
    T: 'static,
{
    /// Declares how DOM-backed state is created and which DOM element is
    /// mounted as its content object.
    pub fn new(
        name: impl Into<String>,
        create: impl Fn(&web_sys::Document, WebEventEmitter) -> Result<T, wasm_bindgen::JsValue>
        + 'static,
        element: impl Fn(&T) -> web_sys::Element + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            create: Rc::new(create),
            element: Rc::new(element),
            properties: HashMap::new(),
            events: HashSet::new(),
            commands: HashMap::new(),
            plain_text: false,
            scroll_content: false,
        }
    }

    /// Declares that this DOM implementation consumes normalized plain text.
    pub fn plain_text(mut self) -> Self {
        self.plain_text = true;
        self
    }

    /// Declares the Host-local scroll presentation used by this element.
    pub fn scroll_container(mut self) -> Self {
        self.scroll_content = true;
        self
    }

    /// Declares one property by its stable Host name.
    pub fn prop(
        mut self,
        property: impl Into<String>,
        set: impl Fn(&mut T, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> + 'static,
        clear: impl Fn(&mut T) -> Result<(), wasm_bindgen::JsValue> + 'static,
    ) -> Self {
        let property = property.into();
        assert!(!property.trim().is_empty(), "Web property name is empty");
        assert!(
            self.properties
                .insert(
                    property.clone(),
                    WebPropBinding {
                        set: Rc::new(set),
                        clear: Rc::new(clear),
                    },
                )
                .is_none(),
            "duplicate Web property binding for {property}"
        );
        self
    }

    /// Declares one event by its stable Host name.
    pub fn event(mut self, event: impl Into<String>) -> Self {
        let event = event.into();
        assert!(!event.trim().is_empty(), "Web event name is empty");
        assert!(
            self.events.insert(event.clone()),
            "duplicate Web event binding for {event}"
        );
        self
    }

    /// Declares one command by its stable Host name.
    pub fn command(
        mut self,
        command: impl Into<String>,
        handler: impl Fn(&mut T, &WhiskerValue) -> Result<(), wasm_bindgen::JsValue> + 'static,
    ) -> Self {
        let command = command.into();
        assert!(!command.trim().is_empty(), "Web command name is empty");
        assert!(
            self.commands
                .insert(command.clone(), Rc::new(handler))
                .is_none(),
            "duplicate Web command binding for {command}"
        );
        self
    }

    fn bind(&self, registration: &ElementRegistration) -> Result<WebNativeConstructor, String> {
        if registration.child_policy.accepts_plain_text() != self.plain_text {
            return Err(format!(
                "plain-text policy differs: Host={}, Rust={:?}",
                self.plain_text, registration.child_policy
            ));
        }
        let schema_properties = registration
            .properties
            .iter()
            .map(|property| property.name.clone())
            .collect::<HashSet<_>>();
        let schema_events = registration
            .events
            .iter()
            .map(|event| event.name.clone())
            .collect::<HashSet<_>>();
        let schema_commands = registration
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<HashSet<_>>();
        let declared_properties = self.properties.keys().cloned().collect::<HashSet<_>>();
        let declared_commands = self.commands.keys().cloned().collect::<HashSet<_>>();
        if declared_properties != schema_properties {
            return Err(format!(
                "property declarations differ: Host={declared_properties:?}, Rust={schema_properties:?}"
            ));
        }
        if self.events != schema_events {
            return Err(format!(
                "event declarations differ: Host={:?}, Rust={schema_events:?}",
                self.events
            ));
        }
        if declared_commands != schema_commands {
            return Err(format!(
                "command declarations differ: Host={declared_commands:?}, Rust={schema_commands:?}"
            ));
        }
        let properties = registration
            .properties
            .iter()
            .map(|schema| (schema.property, self.properties[&schema.name].clone()))
            .collect();
        let commands = registration
            .commands
            .iter()
            .map(|schema| (schema.command, Rc::clone(&self.commands[&schema.name])))
            .collect();
        let definition = Rc::new(BoundWebViewDefinition {
            create: Rc::clone(&self.create),
            element: Rc::clone(&self.element),
            properties,
            commands,
        });
        Ok(Rc::new(move |document, emitter| {
            Ok(Box::new(DeclaredWebElement {
                state: (definition.create)(document, emitter)?,
                definition: definition.clone(),
            }))
        }))
    }
}

impl<T> WebViewImplementation for WebViewDefinition<T>
where
    T: 'static,
{
    fn into_web_factory(self) -> WebElementFactory {
        let name = self.name.clone();
        let plain_text = self.plain_text;
        let scroll_content = self.scroll_content;
        WebElementFactory::declared(name, self, plain_text, scroll_content)
    }
}

struct BoundWebViewDefinition<T> {
    create: WebViewConstructor<T>,
    element: WebElementGetter<T>,
    properties: HashMap<PropertyId, WebPropBinding<T>>,
    commands: HashMap<CommandId, WebCommandHandler<T>>,
}

struct DeclaredWebElement<T> {
    state: T,
    definition: Rc<BoundWebViewDefinition<T>>,
}

impl<T> fmt::Debug for DeclaredWebElement<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeclaredWebElement(..)")
    }
}

impl<T> WebNativeElement for DeclaredWebElement<T>
where
    T: 'static,
{
    fn element(&self) -> web_sys::Element {
        (self.definition.element)(&self.state)
    }

    fn set_property(
        &mut self,
        property: PropertyId,
        value: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let binding = self
            .definition
            .properties
            .get(&property)
            .expect("Web Host validates property IDs");
        (binding.set)(&mut self.state, value)
    }

    fn clear_property(&mut self, property: PropertyId) -> Result<(), wasm_bindgen::JsValue> {
        let binding = self
            .definition
            .properties
            .get(&property)
            .expect("Web Host validates property IDs");
        (binding.clear)(&mut self.state)
    }

    fn invoke_command(
        &mut self,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let handler = self
            .definition
            .commands
            .get(&command)
            .expect("Web Host validates command IDs");
        handler(&mut self.state, arguments)
    }
}

/// DOM factory embedded for one element module.
#[derive(Clone)]
pub struct WebElementFactory {
    pub(crate) name: String,
    pub(crate) kind: WebElementFactoryKind,
    pub(crate) text_content: bool,
    pub(crate) scroll_content: bool,
}

impl WebElementFactory {
    /// Creates a DOM factory joined to a Rust schema by element name.
    pub fn new(name: impl Into<String>, tag_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Tag(tag_name.into()),
            text_content: false,
            scroll_content: false,
        }
    }

    /// Creates a DOM-native element whose implementation is owned by its
    /// external module beside the Rust schema.
    pub fn native<F>(name: impl Into<String>, create: F) -> Self
    where
        F: Fn(
                &web_sys::Document,
                WebEventEmitter,
            ) -> Result<Box<dyn WebNativeElement>, wasm_bindgen::JsValue>
            + 'static,
    {
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Native(Rc::new(create)),
            text_content: false,
            scroll_content: false,
        }
    }

    fn declared<T>(
        name: impl Into<String>,
        definition: WebViewDefinition<T>,
        text_content: bool,
        scroll_content: bool,
    ) -> Self
    where
        T: 'static,
    {
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Declared(Rc::new(definition)),
            text_content,
            scroll_content,
        }
    }

    pub(crate) fn bind(&self, registration: &ElementRegistration) -> Result<Self, WebError> {
        if registration.child_policy.accepts_plain_text() != self.text_content {
            return Err(WebError(format!(
                "DOM factory {} plain-text policy differs: Host={}, Rust={:?}",
                registration.name, self.text_content, registration.child_policy
            )));
        }
        let kind = match &self.kind {
            WebElementFactoryKind::Declared(definition) => {
                WebElementFactoryKind::Native(definition.bind(registration).map_err(|reason| {
                    WebError(format!(
                        "DOM factory {} contract mismatch: {reason}",
                        registration.name
                    ))
                })?)
            }
            other => other.clone(),
        };
        Ok(Self {
            name: self.name.clone(),
            kind,
            text_content: self.text_content,
            scroll_content: self.scroll_content,
        })
    }
}

impl fmt::Debug for WebElementFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebElementFactory")
            .field("name", &self.name)
            .field("kind", &self.kind.name())
            .finish()
    }
}

pub(crate) type WebNativeConstructor = Rc<
    dyn Fn(
        &web_sys::Document,
        WebEventEmitter,
    ) -> Result<Box<dyn WebNativeElement>, wasm_bindgen::JsValue>,
>;

pub(crate) trait WebDeclaredFactory {
    fn bind(&self, registration: &ElementRegistration) -> Result<WebNativeConstructor, String>;
}

impl<T> WebDeclaredFactory for WebViewDefinition<T>
where
    T: 'static,
{
    fn bind(&self, registration: &ElementRegistration) -> Result<WebNativeConstructor, String> {
        WebViewDefinition::bind(self, registration)
    }
}

#[derive(Clone)]
pub(crate) enum WebElementFactoryKind {
    Tag(String),
    Native(WebNativeConstructor),
    Declared(Rc<dyn WebDeclaredFactory>),
}

impl WebElementFactoryKind {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Tag(_) => "tag",
            Self::Native(_) => "native",
            Self::Declared(_) => "declared-native",
        }
    }
}

impl fmt::Debug for WebElementFactoryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag(tag) => formatter.debug_tuple("Tag").field(tag).finish(),
            Self::Native(_) => formatter.write_str("Native(..)"),
            Self::Declared(_) => formatter.write_str("Declared(..)"),
        }
    }
}

/// Event emitted by one module-owned DOM element.
#[derive(Clone, Debug, PartialEq)]
pub struct WebNativeEvent {
    /// Stable event name declared by the Host module.
    pub event: String,
    /// Typed detail routed to the Rust listener.
    pub detail: WhiskerValue,
}

/// Cloneable event channel handed to each DOM-native element instance.
#[derive(Clone)]
pub struct WebEventEmitter(pub(crate) Rc<dyn Fn(WebNativeEvent)>);

impl WebEventEmitter {
    /// Emits an event after the browser callback returns, at the next runtime
    /// frame boundary.
    pub fn emit(&self, event: WebNativeEvent) {
        (self.0)(event);
    }
}

impl fmt::Debug for WebEventEmitter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebEventEmitter(..)")
    }
}

/// Per-node DOM implementation supplied by an external element module.
///
/// Implementations retain their browser listener closures and other resources;
/// dropping the instance during `DeleteNode` releases those resources.
pub trait WebNativeElement: fmt::Debug + 'static {
    /// Returns the DOM element mounted into the common Host projection.
    fn element(&self) -> web_sys::Element;

    /// Applies one schema-validated property.
    fn set_property(
        &mut self,
        property: PropertyId,
        value: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue>;

    /// Restores one schema-validated property to its implementation default.
    fn clear_property(&mut self, property: PropertyId) -> Result<(), wasm_bindgen::JsValue>;

    /// Executes one schema-validated command.
    fn invoke_command(
        &mut self,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue>;
}

/// Built-in element implementations contributed by the Web platform.
pub struct BuiltInElementModule;

#[WhiskerModule]
impl WhiskerModule for BuiltInElementModule {
    type Definition = WebModuleDefinition;

    fn definition() -> Self::Definition {
        fn div(
            document: &web_sys::Document,
            _events: WebEventEmitter,
        ) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
            document.create_element("div")
        }

        WebModuleDefinition::new()
            .view(WebViewDefinition::new("whisker.ui/View", div, Clone::clone))
            .view(WebViewDefinition::new("whisker.ui/Text", div, Clone::clone).plain_text())
            .view(
                WebViewDefinition::new("whisker.ui/ScrollView", div, Clone::clone)
                    .scroll_container(),
            )
    }
}

#[cfg(test)]
pub(crate) fn built_in_element_factories() -> Vec<WebElementFactory> {
    BuiltInElementModule::definition().into_factories()
}

#[cfg(test)]
mod element_registry_tests {
    use super::*;
    use crate::scene::element_registry::DomElementRegistry;
    use whisker::ElementRegistry;
    use whisker_protocol::{
        ElementMeasurement, ElementPropertySchema, ElementSchema, ElementTypeId, ElementValueKind,
    };

    #[test]
    fn built_in_and_package_modules_use_the_same_dom_binding_path() {
        let built_ins = BuiltInElementModule::definition();
        assert_eq!(built_ins.factories().len(), 3);
        let modules = built_ins.view(WebElementFactory::new("whisker.test/Badge", "span"));
        let elements = ElementRegistry::standard_builder()
            .register_provider(whisker::ElementProviderMetadata::named(ElementSchema {
                name: "whisker.test/Badge".into(),
                child_policy: whisker_protocol::ChildPolicy::Elements,
                measurement: ElementMeasurement::None,
                properties: Vec::new(),
                events: Vec::new(),
                commands: Vec::new(),
            }))
            .build()
            .unwrap();
        let factories = modules.factories().to_vec();

        let registry = DomElementRegistry::bind(elements.registrations(), &factories).unwrap();
        let badge = elements
            .registration_for_name("whisker.test/Badge")
            .unwrap();
        assert!(matches!(
            &registry.binding(badge.element_type).unwrap().factory,
            WebElementFactoryKind::Tag(tag_name) if tag_name == "span"
        ));
    }

    #[test]
    fn missing_or_unmatched_dom_factories_fail_bootstrap() {
        let registrations = ElementRegistry::standard().registrations().to_vec();
        let missing = DomElementRegistry::bind(&registrations, &[]).unwrap_err();
        assert!(missing.0.contains("missing DOM factory"));

        let mut factories = built_in_element_factories();
        factories.push(WebElementFactory::new("whisker.test/Unknown", "div"));
        let unknown = DomElementRegistry::bind(&registrations, &factories).unwrap_err();
        assert!(unknown.0.contains("has no Rust element schema"));
    }

    #[test]
    fn declared_host_members_must_match_the_rust_schema_at_bootstrap() {
        let registration = ElementRegistration {
            element_type: ElementTypeId::new(20).unwrap(),
            name: "whisker.test/Toggle".into(),
            child_policy: whisker_protocol::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            properties: vec![ElementPropertySchema {
                property: PropertyId::new(1).unwrap(),
                name: "checked".into(),
                value: ElementValueKind::Bool,
            }],
            events: Vec::new(),
            commands: Vec::new(),
        };
        let definition = WebViewDefinition::new(
            "whisker.test/Toggle",
            |_, _| Ok(()),
            |_| unreachable!("the factory is not instantiated during bootstrap"),
        )
        .prop("misspelled", |_, _| Ok(()), |_| Ok(()));
        let factory = definition.into_web_factory();

        let error = DomElementRegistry::bind(&[registration], &[factory]).unwrap_err();
        assert!(error.0.contains("contract mismatch"));
        assert!(error.0.contains("misspelled"));
        assert!(error.0.contains("checked"));
    }
}
