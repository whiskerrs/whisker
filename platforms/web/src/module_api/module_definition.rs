use super::*;

/// Rust/Web counterpart of the Swift/Kotlin `ModuleDefinition` DSL.
#[derive(Clone, Debug, Default)]
pub struct WebModuleDefinition {
    service: RustModuleDefinition,
    factories: Vec<WebElementFactory>,
}

/// Web Host module declaration, named consistently with native Hosts.
pub type ModuleDefinition = WebModuleDefinition;

impl WebModuleDefinition {
    /// Starts an empty Web module declaration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the package-qualified service module name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.service = self.service.name(name);
        self
    }

    /// Declares one synchronous service function.
    pub fn function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], &ModuleEventEmitter) -> WhiskerValue + 'static,
    ) -> Self {
        self.service = self.service.function(name, handler);
        self
    }

    /// Declares one deferred service function.
    pub fn async_function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], ModulePromise, &ModuleEventEmitter) + 'static,
    ) -> Self {
        self.service = self.service.async_function(name, handler);
        self
    }

    /// Declares one service-scoped event.
    pub fn event(mut self, name: impl Into<String>) -> Self {
        self.service = self.service.event(name);
        self
    }

    /// Declares the first-subscriber hook for one service event.
    pub fn on_start_observing(
        mut self,
        event: impl Into<String>,
        hook: impl Fn(&ModuleEventEmitter) + 'static,
    ) -> Self {
        self.service = self.service.on_start_observing(event, hook);
        self
    }

    /// Declares the last-subscriber hook for one service event.
    pub fn on_stop_observing(
        mut self,
        event: impl Into<String>,
        hook: impl Fn(&ModuleEventEmitter) + 'static,
    ) -> Self {
        self.service = self.service.on_stop_observing(event, hook);
        self
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

    /// Returns the portable service declaration bound by the browser runtime.
    pub fn service_definition(&self) -> &RustModuleDefinition {
        &self.service
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
type WebTextStyleUpdater<T> =
    Rc<dyn Fn(&mut T, &WhiskerTextStyle) -> Result<(), wasm_bindgen::JsValue>>;
pub(crate) type WebMeasurementHandler =
    Rc<dyn Fn(&WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize>>;

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
    text_style: Option<WebTextStyleUpdater<T>>,
    pub(super) measurement: Option<WebMeasurementHandler>,
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
            text_style: None,
            measurement: None,
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

    /// Declares that this DOM content object consumes resolved inherited text
    /// style independently from plain-text children.
    pub fn text_style(
        mut self,
        update: impl Fn(&mut T, &WhiskerTextStyle) -> Result<(), wasm_bindgen::JsValue> + 'static,
    ) -> Self {
        assert!(
            self.text_style.replace(Rc::new(update)).is_none(),
            "duplicate Web TextStyle binding for {}",
            self.name
        );
        self
    }

    /// Supplies synchronous Host intrinsic measurement for Custom or
    /// ReplacedContent schemas. `None` means unsupported for this request.
    pub fn measurement(
        mut self,
        measure: impl Fn(&WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize> + 'static,
    ) -> Self {
        assert!(
            self.measurement.replace(Rc::new(measure)).is_none(),
            "duplicate Web Measurement binding for {}",
            self.name
        );
        self
    }

    pub(super) fn bind(
        &self,
        registration: &ElementRegistration,
    ) -> Result<WebNativeConstructor, String> {
        if registration.child_policy.accepts_plain_text() != self.plain_text {
            return Err(format!(
                "plain-text policy differs: Host={}, Rust={:?}",
                self.plain_text, registration.child_policy
            ));
        }
        if registration.text_style != self.text_style.is_some() {
            return Err(format!(
                "text-style capability differs: Host={}, Rust={}",
                self.text_style.is_some(),
                registration.text_style
            ));
        }
        let needs_host_measurement = matches!(
            registration.measurement,
            whisker_protocol::ElementMeasurement::ReplacedContent
                | whisker_protocol::ElementMeasurement::Custom
        );
        if needs_host_measurement != self.measurement.is_some() {
            return Err(format!(
                "measurement capability differs: Host={}, Rust={:?}",
                self.measurement.is_some(),
                registration.measurement
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
            text_style: self.text_style.clone(),
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
    text_style: Option<WebTextStyleUpdater<T>>,
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

    fn set_text_style(&mut self, style: &WhiskerTextStyle) -> Result<(), wasm_bindgen::JsValue> {
        match &self.definition.text_style {
            Some(update) => update(&mut self.state, style),
            None => Ok(()),
        }
    }
}
