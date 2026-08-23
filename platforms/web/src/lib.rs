//! Browser DOM Host for Whisker.
//!
//! Rust remains authoritative for style resolution and Taffy layout. This Host
//! synchronously measures browser text and applies the resulting semantic frame
//! transaction to DOM nodes using explicit geometry.

#![warn(missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use whisker::runtime::RuntimeWakeHandle;
use whisker::{Element, ElementModuleDefinition, ElementRegistry, RuntimeInstance, SurfaceRuntime};
use whisker_engine::{FrameSink, LayoutOptions, MeasurementProvider};
use whisker_protocol::{
    ApplyResult, AvailableSpace, BorderLineStyle, CommandId, ElementRegistration, ElementTypeId,
    FrameMode, FramePacket, InputEvent, InputEventKind, MeasureFontFamily, MeasureFontStyle,
    MeasureLineHeight, MeasureTextDirection, MeasureTextWrap, MeasuredSize, MeasurementMetrics,
    MeasurementPayload, MeasurementRequest, MeasurementResponse, NodeId, Operation, OverflowClip,
    PaintColor, PaintCornerRadius, PaintLengthPercentage, PreparedContentId, PropertyId,
    SceneProjection, SurfaceId, UnsupportedMeasurementReason,
};
use whisker_style::StyleEnvironment;

/// Marks and defines a platform implementation contributed by a module.
pub use whisker::WhiskerModule;

/// Browser bindings used by Rust-authored Web Host contributions.
pub use wasm_bindgen;
/// DOM bindings used by Rust-authored Web Host contributions.
pub use web_sys;
/// Shared value used by Web module properties, functions, and events.
pub use whisker_value::WhiskerValue;

thread_local! {
    static APPLICATION: RefCell<Option<WebApplication>> = const { RefCell::new(None) };
    static FRAME_SCHEDULED: Cell<bool> = const { Cell::new(false) };
}

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
    name: String,
    kind: WebElementFactoryKind,
    text_content: bool,
    scroll_content: bool,
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

    fn bind(&self, registration: &ElementRegistration) -> Result<Self, WebError> {
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

type WebNativeConstructor = Rc<
    dyn Fn(
        &web_sys::Document,
        WebEventEmitter,
    ) -> Result<Box<dyn WebNativeElement>, wasm_bindgen::JsValue>,
>;

trait WebDeclaredFactory {
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
enum WebElementFactoryKind {
    Tag(String),
    Native(WebNativeConstructor),
    Declared(Rc<dyn WebDeclaredFactory>),
}

impl WebElementFactoryKind {
    fn name(&self) -> &'static str {
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
pub struct WebEventEmitter(Rc<dyn Fn(WebNativeEvent)>);

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
fn built_in_element_factories() -> Vec<WebElementFactory> {
    BuiltInElementModule::definition().into_factories()
}

/// Failure while creating or driving the browser Host.
#[derive(Clone, Debug)]
pub struct WebError(String);

impl fmt::Display for WebError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for WebError {}

/// Mounts a Whisker application into the current browser document.
///
/// The generated `gen/web` crate calls this once from its WASM start
/// function. Subsequent work is driven by `requestAnimationFrame`.
pub fn run(config: WebAppConfig, application: fn() -> Element) -> Result<(), WebError> {
    APPLICATION.with(|slot| {
        if slot.borrow().is_some() {
            return Err(WebError("a Web application is already mounted".into()));
        }
        *slot.borrow_mut() = Some(WebApplication::new(config)?);
        Ok(())
    })?;

    let mount = APPLICATION.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.as_mut()
            .expect("application was installed")
            .runtime
            .mount(application)
            .map(|_| ())
            .map_err(|error| WebError(format!("mount Whisker application: {error}")))
    });
    if let Err(error) = mount {
        APPLICATION.with(|slot| *slot.borrow_mut() = None);
        return Err(error);
    }

    let resize = Closure::<dyn FnMut()>::new(request_frame);
    browser_window()?
        .add_event_listener_with_callback("resize", resize.as_ref().unchecked_ref())
        .map_err(|error| js_error("register resize listener", error))?;
    resize.forget();
    request_frame();
    Ok(())
}

struct WebApplication {
    runtime: RuntimeInstance,
    measurements: DomMeasurementProvider,
    frames: DomFrameSink,
    viewport: (f32, f32, f32),
    viewport_epoch: u32,
    environment_epoch: u64,
}

impl WebApplication {
    fn new(mut config: WebAppConfig) -> Result<Self, WebError> {
        let mut element_factories = BuiltInElementModule::definition().into_factories();
        let elements = ElementRegistry::standard_builder()
            .register_modules(config.element_modules.drain(..))
            .build()
            .map_err(|error| WebError(format!("build element registry: {error}")))?;
        element_factories.extend(
            config
                .module_definitions
                .drain(..)
                .flat_map(WebModuleDefinition::into_factories),
        );
        let window = browser_window()?;
        let document = window
            .document()
            .ok_or_else(|| WebError("browser document is unavailable".into()))?;
        document.set_title(&config.title);
        let root = document
            .get_element_by_id(&config.root_id)
            .ok_or_else(|| WebError(format!("missing Web Host root #{}", config.root_id)))?;
        set_style(&root, "position", "relative")?;
        set_style(&root, "width", "100vw")?;
        set_style(&root, "height", "100vh")?;
        set_style(&root, "overflow", "hidden")?;

        let viewport = viewport(&window)?;
        let surface_id = SurfaceId::new(1).expect("the browser surface id is non-zero");
        let registrations = elements.registrations().to_vec();
        let surface = SurfaceRuntime::with_element_registry(
            surface_id,
            StyleEnvironment::new(viewport.0, viewport.1, viewport.2, 16.0),
            elements,
        );
        let wake = RuntimeWakeHandle::new(request_frame);
        Ok(Self {
            runtime: RuntimeInstance::new(surface, wake),
            measurements: DomMeasurementProvider::new(document.clone()),
            frames: DomFrameSink::new(
                document,
                root,
                surface_id,
                &registrations,
                &element_factories,
            )?,
            viewport,
            viewport_epoch: 1,
            environment_epoch: 1,
        })
    }

    fn drive_frame(&mut self, timestamp_ms: f64) -> Result<(), WebError> {
        for event in self.frames.take_events() {
            self.runtime
                .dispatch_input(&InputEvent {
                    surface: self.runtime.surface().surface(),
                    timestamp_ms,
                    kind: InputEventKind::Named(event.name),
                    pointer: None,
                    target: Some(event.target),
                    detail: event.detail,
                })
                .map_err(|error| WebError(format!("dispatch Web provider event: {error}")))?;
        }
        let current = viewport(&browser_window()?)?;
        if current != self.viewport {
            self.viewport = current;
            self.viewport_epoch = self.viewport_epoch.wrapping_add(1).max(1);
            self.environment_epoch = self.environment_epoch.wrapping_add(1).max(1);
        }
        let drive = self
            .runtime
            .drive_frame(
                timestamp_ms,
                StyleEnvironment::new(self.viewport.0, self.viewport.1, self.viewport.2, 16.0),
                self.environment_epoch,
                self.viewport_epoch,
                &mut self.measurements,
                &mut self.frames,
                LayoutOptions::default(),
            )
            .map_err(|error| WebError(format!("drive Web frame: {error}")))?;
        if drive.needs_frame {
            request_frame();
        }
        Ok(())
    }
}

fn request_frame() {
    FRAME_SCHEDULED.with(|scheduled| {
        if scheduled.replace(true) {
            return;
        }
        let callback = Closure::once(move |timestamp_ms: f64| {
            FRAME_SCHEDULED.with(|scheduled| scheduled.set(false));
            let result = APPLICATION.with(|slot| {
                let mut slot = slot.borrow_mut();
                slot.as_mut()
                    .ok_or_else(|| WebError("Web application is not mounted".into()))?
                    .drive_frame(timestamp_ms)
            });
            if let Err(error) = result {
                web_sys::console::error_1(&error.to_string().into());
            }
        });
        match web_sys::window().and_then(|window| {
            window
                .request_animation_frame(callback.as_ref().unchecked_ref())
                .ok()
        }) {
            Some(_) => callback.forget(),
            None => scheduled.set(false),
        }
    });
}

fn browser_window() -> Result<web_sys::Window, WebError> {
    web_sys::window().ok_or_else(|| WebError("browser window is unavailable".into()))
}

fn viewport(window: &web_sys::Window) -> Result<(f32, f32, f32), WebError> {
    let width = window
        .inner_width()
        .map_err(|error| js_error("read viewport width", error))?
        .as_f64()
        .ok_or_else(|| WebError("viewport width was not numeric".into()))? as f32;
    let height = window
        .inner_height()
        .map_err(|error| js_error("read viewport height", error))?
        .as_f64()
        .ok_or_else(|| WebError("viewport height was not numeric".into()))? as f32;
    Ok((width, height, window.device_pixel_ratio() as f32))
}

struct DomMeasurementProvider {
    document: web_sys::Document,
}

impl DomMeasurementProvider {
    fn new(document: web_sys::Document) -> Self {
        Self { document }
    }
}

impl MeasurementProvider for DomMeasurementProvider {
    type Error = WebError;

    fn measure_batch(
        &mut self,
        _surface: SurfaceId,
        requests: &[MeasurementRequest],
        responses: &mut Vec<MeasurementResponse>,
    ) -> Result<(), Self::Error> {
        let body = self
            .document
            .body()
            .ok_or_else(|| WebError("document body is unavailable".into()))?;
        for request in requests {
            let MeasurementPayload::Text(text) = &request.payload else {
                responses.push(MeasurementResponse::Ready {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    metrics: MeasurementMetrics {
                        size: MeasuredSize::new(0.0, 0.0),
                        first_baseline: None,
                        last_baseline: None,
                        overflow: None,
                        prepared_content: None,
                    },
                });
                continue;
            };
            if text.style.uses_extended_typography() {
                responses.push(MeasurementResponse::Unsupported {
                    key: request.key,
                    environment_epoch: request.environment_epoch,
                    reason: UnsupportedMeasurementReason::Feature,
                });
                continue;
            }
            let probe = self
                .document
                .create_element("div")
                .map_err(|error| js_error("create text measurement probe", error))?;
            set_style(&probe, "position", "absolute")?;
            set_style(&probe, "visibility", "hidden")?;
            set_style(&probe, "pointer-events", "none")?;
            set_style(&probe, "left", "-100000px")?;
            set_style(&probe, "top", "0")?;
            set_style(&probe, "box-sizing", "border-box")?;
            apply_text_metrics_style(&probe, text)?;
            match request.constraints.available_space[0] {
                AvailableSpace::Definite(width) => {
                    set_style(&probe, "width", &px(width.max(0.0)))?;
                }
                AvailableSpace::MinContent => set_style(&probe, "width", "min-content")?,
                AvailableSpace::MaxContent => set_style(&probe, "width", "max-content")?,
            }
            if let Some(width) = request.constraints.known_dimensions[0] {
                set_style(&probe, "width", &px(width))?;
            }
            if let Some(height) = request.constraints.known_dimensions[1] {
                set_style(&probe, "height", &px(height))?;
            }
            probe.set_text_content(Some(&text.text));
            body.append_child(&probe)
                .map_err(|error| js_error("attach text measurement probe", error))?;
            let rect = probe.get_bounding_client_rect();
            probe.remove();
            let baseline = text.style.font_size * 0.8;
            responses.push(MeasurementResponse::Ready {
                key: request.key,
                environment_epoch: request.environment_epoch,
                metrics: MeasurementMetrics {
                    size: MeasuredSize::new(rect.width() as f32, rect.height() as f32),
                    first_baseline: Some(baseline),
                    last_baseline: Some(baseline),
                    overflow: None,
                    prepared_content: PreparedContentId::new(request.key.get()),
                },
            });
        }
        Ok(())
    }
}

struct DomFrameSink {
    document: web_sys::Document,
    root: web_sys::Element,
    projection: SceneProjection,
    elements: DomElementRegistry,
    nodes: HashMap<NodeId, web_sys::Element>,
    node_types: HashMap<NodeId, ElementTypeId>,
    parents: HashMap<NodeId, NodeId>,
    layouts: HashMap<NodeId, whisker_protocol::LayoutGeometry>,
    text_nodes: HashMap<NodeId, web_sys::Element>,
    native_nodes: HashMap<NodeId, Box<dyn WebNativeElement>>,
    event_masks: HashMap<NodeId, Rc<Cell<u64>>>,
    pending_events: Rc<RefCell<VecDeque<WebProviderEvent>>>,
}

#[derive(Clone, Debug, PartialEq)]
struct WebProviderEvent {
    target: NodeId,
    name: String,
    detail: WhiskerValue,
}

#[derive(Clone, Debug)]
struct DomElementRegistry {
    bindings: HashMap<ElementTypeId, DomElementBinding>,
}

#[derive(Clone, Debug)]
struct DomElementBinding {
    registration: ElementRegistration,
    factory: WebElementFactoryKind,
    text_content: bool,
    scroll_content: bool,
}

impl DomElementRegistry {
    fn bind(
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

    fn binding(&self, element_type: ElementTypeId) -> Result<&DomElementBinding, WebError> {
        self.bindings.get(&element_type).ok_or_else(|| {
            WebError(format!(
                "DOM Host received unknown element type {}",
                element_type.get()
            ))
        })
    }
}

impl DomFrameSink {
    fn new(
        document: web_sys::Document,
        root: web_sys::Element,
        surface: SurfaceId,
        registrations: &[ElementRegistration],
        factories: &[WebElementFactory],
    ) -> Result<Self, WebError> {
        Ok(Self {
            document,
            root,
            projection: SceneProjection::new(surface),
            elements: DomElementRegistry::bind(registrations, factories)?,
            nodes: HashMap::new(),
            node_types: HashMap::new(),
            parents: HashMap::new(),
            layouts: HashMap::new(),
            text_nodes: HashMap::new(),
            native_nodes: HashMap::new(),
            event_masks: HashMap::new(),
            pending_events: Rc::new(RefCell::new(VecDeque::new())),
        })
    }

    fn take_events(&self) -> Vec<WebProviderEvent> {
        self.pending_events.borrow_mut().drain(..).collect()
    }

    fn apply(&mut self, packet: &FramePacket) -> Result<(), WebError> {
        if let Some(feature) = packet
            .operations
            .iter()
            .find_map(|operation| match operation {
                Operation::SetBackgroundLayers { .. } => Some("background-layers"),
                Operation::SetVisualEffects { .. } => Some("visual-effects"),
                Operation::SetImage { .. } => Some("image-content"),
                Operation::SetCursor { .. } => Some("cursor"),
                Operation::SetText { content, .. } if content.paint.uses_extended_features() => {
                    Some("text-effects")
                }
                Operation::SetText { content, .. }
                    if content.payload.style.uses_extended_typography() =>
                {
                    Some("text-typography")
                }
                _ => None,
            })
        {
            return Err(WebError(format!(
                "DOM Host does not implement protocol feature {feature}"
            )));
        }
        if packet.header.mode == FrameMode::Snapshot {
            self.root.set_inner_html("");
            self.nodes.clear();
            self.node_types.clear();
            self.parents.clear();
            self.layouts.clear();
            self.text_nodes.clear();
            self.native_nodes.clear();
            self.event_masks.clear();
            self.pending_events.borrow_mut().clear();
        }
        for operation in &packet.operations {
            self.apply_operation(operation)?;
        }
        Ok(())
    }

    fn apply_operation(&mut self, operation: &Operation) -> Result<(), WebError> {
        match operation {
            Operation::CreateNode { node, element_type } => {
                let binding = self.elements.binding(*element_type)?.clone();
                let event_mask = Rc::new(Cell::new(0));
                let emitter = WebEventEmitter({
                    let registration = binding.registration.clone();
                    let event_mask = Rc::clone(&event_mask);
                    let pending = Rc::clone(&self.pending_events);
                    let node = *node;
                    Rc::new(move |event: WebNativeEvent| {
                        let Some(schema) = registration.event_named(&event.event) else {
                            web_sys::console::error_1(
                                &format!(
                                    "DOM element {} emitted unknown event {}",
                                    registration.name, event.event
                                )
                                .into(),
                            );
                            return;
                        };
                        if !schema.accepts_detail(&event.detail) {
                            web_sys::console::error_1(
                                &format!(
                                    "DOM element {} emitted invalid detail for {}",
                                    registration.name, schema.name
                                )
                                .into(),
                            );
                            return;
                        }
                        let mask = schema
                            .mask()
                            .expect("registration validation checked event ID");
                        if event_mask.get() & mask == 0 {
                            return;
                        }
                        pending.borrow_mut().push_back(WebProviderEvent {
                            target: node,
                            name: schema.name.clone(),
                            detail: event.detail,
                        });
                        request_frame();
                    })
                });
                let (element, native) = match &binding.factory {
                    WebElementFactoryKind::Tag(tag_name) => (
                        self.document
                            .create_element(tag_name)
                            .map_err(|error| js_error("create Whisker DOM node", error))?,
                        None,
                    ),
                    WebElementFactoryKind::Native(create) => {
                        let native = create(&self.document, emitter)
                            .map_err(|error| js_error("create native Whisker DOM node", error))?;
                        (native.element(), Some(native))
                    }
                    WebElementFactoryKind::Declared(_) => {
                        unreachable!("DOM declared factory was not bound at bootstrap")
                    }
                };
                element
                    .set_attribute("data-whisker-node", &node.get().to_string())
                    .map_err(|error| js_error("mark Whisker DOM node", error))?;
                element
                    .set_attribute("data-whisker-content", binding.factory.name())
                    .map_err(|error| js_error("mark Whisker DOM element content", error))?;
                set_style(&element, "position", "absolute")?;
                set_style(&element, "box-sizing", "border-box")?;
                if binding.scroll_content {
                    set_style(&element, "overflow-x", "hidden")?;
                    set_style(&element, "overflow-y", "auto")?;
                }
                self.root
                    .append_child(&element)
                    .map_err(|error| js_error("attach Whisker DOM node", error))?;
                self.nodes.insert(*node, element);
                self.node_types.insert(*node, *element_type);
                self.event_masks.insert(*node, event_mask);
                if let Some(native) = native {
                    self.native_nodes.insert(*node, native);
                }
            }
            Operation::DeleteNode { node } => self.delete_subtree(*node),
            Operation::InsertChild {
                parent,
                child,
                index,
            }
            | Operation::MoveChild {
                parent,
                child,
                index,
            } => {
                let parent_element = self.node(*parent)?;
                let child_element = self.node(*child)?;
                let reference = parent_element.children().item(*index);
                parent_element
                    .insert_before(&child_element, reference.as_ref().map(AsRef::as_ref))
                    .map_err(|error| js_error("insert Whisker DOM child", error))?;
                self.parents.insert(*child, *parent);
            }
            Operation::RemoveChild { parent: _, child } => {
                if let Some(element) = self.nodes.get(child) {
                    element.remove();
                }
                self.parents.remove(child);
            }
            Operation::SetLayout { node, geometry } => {
                let element = self.node(*node)?;
                let rect = geometry.border_box;
                set_style(&element, "left", &px(rect.x))?;
                set_style(&element, "top", &px(rect.y))?;
                set_style(&element, "width", &px(rect.width))?;
                set_style(&element, "height", &px(rect.height))?;
                self.layouts.insert(*node, *geometry);
                if let Some(text) = self.text_nodes.get(node) {
                    position_text(text, geometry.content_box)?;
                }
            }
            Operation::SetBoxPaint { node, paint } => {
                let element = self.node(*node)?;
                set_style(
                    &element,
                    "background-color",
                    &color(&paint.background_color),
                )?;
                let widths = &paint.border_widths;
                set_style(&element, "border-top-width", &length(widths.top))?;
                set_style(&element, "border-right-width", &length(widths.right))?;
                set_style(&element, "border-bottom-width", &length(widths.bottom))?;
                set_style(&element, "border-left-width", &length(widths.left))?;
                let colors = &paint.border_colors;
                set_style(&element, "border-top-color", &color(&colors.top))?;
                set_style(&element, "border-right-color", &color(&colors.right))?;
                set_style(&element, "border-bottom-color", &color(&colors.bottom))?;
                set_style(&element, "border-left-color", &color(&colors.left))?;
                let styles = &paint.border_styles;
                set_style(&element, "border-top-style", border_style(styles.top))?;
                set_style(&element, "border-right-style", border_style(styles.right))?;
                set_style(&element, "border-bottom-style", border_style(styles.bottom))?;
                set_style(&element, "border-left-style", border_style(styles.left))?;
                let radii = &paint.border_radii;
                set_style(
                    &element,
                    "border-top-left-radius",
                    &corner_radius(radii.top_left),
                )?;
                set_style(
                    &element,
                    "border-top-right-radius",
                    &corner_radius(radii.top_right),
                )?;
                set_style(
                    &element,
                    "border-bottom-right-radius",
                    &corner_radius(radii.bottom_right),
                )?;
                set_style(
                    &element,
                    "border-bottom-left-radius",
                    &corner_radius(radii.bottom_left),
                )?;
            }
            Operation::SetClip { node, clip } => {
                let element = self.node(*node)?;
                let element_type = *self
                    .node_types
                    .get(node)
                    .ok_or_else(|| WebError(format!("missing DOM element type for {node:?}")))?;
                let scroll_content = self.elements.binding(element_type)?.scroll_content;
                set_style(
                    &element,
                    "overflow-x",
                    if clip.horizontal == OverflowClip::Hidden {
                        "hidden"
                    } else if scroll_content {
                        "auto"
                    } else {
                        "visible"
                    },
                )?;
                set_style(
                    &element,
                    "overflow-y",
                    if clip.vertical == OverflowClip::Hidden {
                        "hidden"
                    } else if scroll_content {
                        "auto"
                    } else {
                        "visible"
                    },
                )?;
            }
            Operation::SetTransform { node, transform } => {
                let value = transform
                    .0
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                set_style(
                    &self.node(*node)?,
                    "transform",
                    &format!("matrix3d({value})"),
                )?;
            }
            Operation::SetOpacity { node, opacity } => {
                set_style(&self.node(*node)?, "opacity", &opacity.to_string())?;
            }
            Operation::SetVisibility { node, visibility } => {
                set_style(
                    &self.node(*node)?,
                    "visibility",
                    if matches!(visibility, whisker_protocol::Visibility::Visible) {
                        "visible"
                    } else {
                        "hidden"
                    },
                )?;
            }
            Operation::SetZOrder { node, z_order } => {
                set_style(&self.node(*node)?, "z-index", &z_order.to_string())?;
            }
            Operation::SetText { node, content } => {
                let element_type = self.node_types.get(node).copied().ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                if !self.elements.binding(element_type)?.text_content {
                    return Err(WebError(format!(
                        "DOM Host received text for non-text node {}",
                        node.get()
                    )));
                }
                let text = if let Some(text) = self.text_nodes.get(node) {
                    text.clone()
                } else {
                    let text = self
                        .document
                        .create_element("span")
                        .map_err(|error| js_error("create Whisker DOM text", error))?;
                    text.set_attribute("data-whisker-text", "")
                        .map_err(|error| js_error("mark Whisker DOM text", error))?;
                    set_style(&text, "position", "absolute")?;
                    self.node(*node)?
                        .append_child(&text)
                        .map_err(|error| js_error("attach Whisker DOM text", error))?;
                    self.text_nodes.insert(*node, text.clone());
                    text
                };
                if let Some(geometry) = self.layouts.get(node) {
                    position_text(&text, geometry.content_box)?;
                }
                apply_text_metrics_style(&text, &content.payload)?;
                set_style(&text, "color", &color(&content.paint.foreground))?;
                text.set_text_content(Some(&content.payload.text));
            }
            Operation::SetHitTest { node, behavior } => {
                let disabled = matches!(
                    behavior,
                    whisker_protocol::HitTestBehavior::None
                        | whisker_protocol::HitTestBehavior::DescendantsOnly
                );
                set_style(
                    &self.node(*node)?,
                    "pointer-events",
                    if disabled { "none" } else { "auto" },
                )?;
            }
            Operation::SetProperty {
                node,
                property,
                value,
            } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                let schema = registration.property(*property).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no property {}",
                        registration.name,
                        property.get()
                    ))
                })?;
                if !schema.value.accepts(value) {
                    return Err(WebError(format!(
                        "DOM property {} expected {:?}",
                        schema.name, schema.value
                    )));
                }
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .set_property(*property, value)
                    .map_err(|error| js_error("set native DOM property", error))?;
            }
            Operation::ClearProperty { node, property } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                registration.property(*property).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no property {}",
                        registration.name,
                        property.get()
                    ))
                })?;
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .clear_property(*property)
                    .map_err(|error| js_error("clear native DOM property", error))?;
            }
            Operation::SetEventMask { node, event_mask } => {
                self.event_masks
                    .get(node)
                    .ok_or_else(|| {
                        WebError(format!("DOM projection is missing node {}", node.get()))
                    })?
                    .set(*event_mask);
            }
            Operation::InvokeCommand {
                node,
                command,
                arguments,
                ..
            } => {
                let element_type = *self.node_types.get(node).ok_or_else(|| {
                    WebError(format!("DOM projection is missing node {}", node.get()))
                })?;
                let registration = &self.elements.binding(element_type)?.registration;
                let schema = registration.command(*command).ok_or_else(|| {
                    WebError(format!(
                        "DOM element {} has no command {}",
                        registration.name,
                        command.get()
                    ))
                })?;
                if !schema.arguments.accepts(arguments) {
                    return Err(WebError(format!(
                        "DOM command {} expected {:?}",
                        schema.name, schema.arguments
                    )));
                }
                self.native_nodes
                    .get_mut(node)
                    .ok_or_else(|| WebError(format!("DOM node {} is not native", node.get())))?
                    .invoke_command(*command, arguments)
                    .map_err(|error| js_error("invoke native DOM command", error))?;
            }
            Operation::SetPointerCapture { .. } | Operation::ReleasePointerCapture { .. } => {}
            Operation::SetBackgroundLayers { .. }
            | Operation::SetVisualEffects { .. }
            | Operation::SetImage { .. }
            | Operation::SetCursor { .. } => {
                unreachable!("unsupported operations are rejected before DOM mutation")
            }
        }
        Ok(())
    }

    fn node(&self, node: NodeId) -> Result<web_sys::Element, WebError> {
        self.nodes
            .get(&node)
            .cloned()
            .ok_or_else(|| WebError(format!("DOM projection is missing node {}", node.get())))
    }

    fn delete_subtree(&mut self, root: NodeId) {
        if let Some(element) = self.nodes.get(&root) {
            element.remove();
        }
        let mut deleted = vec![root];
        let mut cursor = 0;
        while cursor < deleted.len() {
            let parent = deleted[cursor];
            deleted.extend(
                self.parents
                    .iter()
                    .filter_map(|(child, candidate)| (*candidate == parent).then_some(*child)),
            );
            cursor += 1;
        }
        for node in deleted {
            self.nodes.remove(&node);
            self.node_types.remove(&node);
            self.parents.remove(&node);
            self.layouts.remove(&node);
            self.text_nodes.remove(&node);
            self.native_nodes.remove(&node);
            self.event_masks.remove(&node);
        }
    }
}

fn position_text(
    element: &web_sys::Element,
    rect: whisker_protocol::LayoutRect,
) -> Result<(), WebError> {
    set_style(element, "left", &px(rect.x))?;
    set_style(element, "top", &px(rect.y))?;
    set_style(element, "width", &px(rect.width))?;
    set_style(element, "height", &px(rect.height))?;
    set_style(element, "overflow", "hidden")
}

impl FrameSink for DomFrameSink {
    type Error = WebError;

    fn capabilities(&self) -> whisker_protocol::RenderCapabilities {
        whisker_protocol::RenderCapabilities::new(
            whisker_protocol::ProtocolVersion::CURRENT,
            [whisker_protocol::CapabilityEntry {
                capability: whisker_protocol::RenderCapability::EllipticalBorderRadius,
                support: whisker_protocol::CapabilitySupport::Native,
            }],
        )
        .expect("Web capability profile is unique")
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        if let Some(capability) = self.capabilities().first_unsupported(packet) {
            return Err(WebError(format!(
                "DOM Host does not implement protocol feature {}",
                capability.as_str()
            )));
        }
        let mut next = self.projection.clone();
        let result = next
            .apply(packet)
            .map_err(|error| WebError(error.to_string()))?;
        if matches!(result, ApplyResult::Accepted { .. }) {
            self.apply(packet)?;
            self.projection = next;
        }
        Ok(result)
    }
}

fn apply_text_metrics_style(
    element: &web_sys::Element,
    text: &whisker_protocol::TextMeasurePayload,
) -> Result<(), WebError> {
    let families = text
        .style
        .font_families
        .iter()
        .map(|family| match family {
            MeasureFontFamily::System => "system-ui".to_string(),
            MeasureFontFamily::Named(name) => format!("{name:?}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    set_style(element, "font-family", &families)?;
    set_style(element, "font-size", &px(text.style.font_size))?;
    set_style(element, "font-weight", &text.style.font_weight.to_string())?;
    set_style(
        element,
        "font-style",
        match text.style.font_style {
            MeasureFontStyle::Normal => "normal",
            MeasureFontStyle::Italic => "italic",
            MeasureFontStyle::Oblique => "oblique",
        },
    )?;
    set_style(
        element,
        "line-height",
        &match text.style.line_height {
            MeasureLineHeight::Normal => "normal".to_string(),
            MeasureLineHeight::LogicalPixels(value) => px(value),
        },
    )?;
    set_style(element, "letter-spacing", &px(text.style.letter_spacing))?;
    set_style(
        element,
        "white-space",
        if text.wrap == MeasureTextWrap::NoWrap {
            "nowrap"
        } else {
            "normal"
        },
    )?;
    set_style(
        element,
        "direction",
        match text.direction {
            MeasureTextDirection::Auto => "initial",
            MeasureTextDirection::LeftToRight => "ltr",
            MeasureTextDirection::RightToLeft => "rtl",
        },
    )?;
    set_style(element, "overflow-wrap", "anywhere")?;
    Ok(())
}

fn set_style(element: &web_sys::Element, property: &str, value: &str) -> Result<(), WebError> {
    let html = element
        .dyn_ref::<web_sys::HtmlElement>()
        .ok_or_else(|| WebError("Whisker DOM node is not an HtmlElement".into()))?;
    html.style()
        .set_property(property, value)
        .map_err(|error| js_error(&format!("set CSS property {property}"), error))
}

fn px(value: f32) -> String {
    format!("{value}px")
}

fn length(value: PaintLengthPercentage) -> String {
    if value.fraction == 0.0 {
        px(value.length)
    } else {
        format!("calc({}px + {}%)", value.length, value.fraction * 100.0)
    }
}

fn corner_radius(value: PaintCornerRadius) -> String {
    format!("{} {}", length(value.horizontal), length(value.vertical))
}

fn color(value: &PaintColor) -> String {
    match value {
        PaintColor::Named(name) => name.clone(),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => format!("rgba({red}, {green}, {blue}, {alpha})"),
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => format!("hsla({hue_degrees}, {saturation}%, {lightness}%, {alpha})"),
    }
}

fn border_style(value: BorderLineStyle) -> &'static str {
    match value {
        BorderLineStyle::None => "none",
        BorderLineStyle::Hidden => "hidden",
        BorderLineStyle::Solid => "solid",
        BorderLineStyle::Dashed => "dashed",
        BorderLineStyle::Dotted => "dotted",
        BorderLineStyle::Double => "double",
        BorderLineStyle::Groove => "groove",
        BorderLineStyle::Ridge => "ridge",
        BorderLineStyle::Inset => "inset",
        BorderLineStyle::Outset => "outset",
    }
}

fn js_error(context: &str, value: wasm_bindgen::JsValue) -> WebError {
    WebError(format!(
        "{context}: {}",
        value.as_string().unwrap_or_else(|| format!("{value:?}"))
    ))
}

#[cfg(test)]
mod element_registry_tests {
    use super::*;
    use whisker::ElementRegistry;
    use whisker_protocol::{
        ElementMeasurement, ElementPropertySchema, ElementSchema, ElementValueKind,
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

#[cfg(all(test, target_arch = "wasm32"))]
#[path = "tests/host_conformance.rs"]
mod host_conformance_tests;
