use super::*;

/// Rust-native counterpart of the Swift/Kotlin `ModuleDefinition` DSL.
#[derive(Clone, Debug, Default)]
pub struct DesktopModuleDefinition {
    service: RustModuleDefinition,
    factories: Vec<DesktopElementFactory>,
}

impl DesktopModuleDefinition {
    /// Starts an empty Desktop module declaration.
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
        I: DesktopViewImplementation,
    {
        self.factories.push(implementation.into_desktop_factory());
        self
    }

    /// Returns every Host factory contributed by this declaration.
    pub fn factories(&self) -> &[DesktopElementFactory] {
        &self.factories
    }

    /// Consumes the declaration and returns its Host factories.
    pub fn into_factories(self) -> Vec<DesktopElementFactory> {
        self.factories
    }

    /// Returns the portable service declaration bound by the Desktop runtime.
    pub fn service_definition(&self) -> &RustModuleDefinition {
        &self.service
    }
}

/// Converts one Desktop `View` declaration into its Host factory.
pub trait DesktopViewImplementation {
    /// Erases this declaration into a factory bound by name at bootstrap.
    fn into_desktop_factory(self) -> DesktopElementFactory;
}

impl DesktopViewImplementation for DesktopElementFactory {
    fn into_desktop_factory(self) -> DesktopElementFactory {
        self
    }
}

type DesktopPropSetter<T> = Arc<dyn Fn(&mut T, &WhiskerValue) + Send + Sync>;
type DesktopPropClearer<T> = Arc<dyn Fn(&mut T) + Send + Sync>;
type DesktopCommandHandler<T> = Arc<dyn Fn(&mut T, &WhiskerValue) + Send + Sync>;
type DesktopTextStyleUpdater<T> = Arc<dyn Fn(&mut T, &WhiskerTextStyle) + Send + Sync>;
pub(super) type DesktopMeasurementHandler =
    Arc<dyn Fn(&WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize> + Send + Sync>;
type DesktopRasterizer<T> = Arc<dyn Fn(&T, u32, u32, f32) -> Option<DesktopRaster> + Send + Sync>;
type DesktopScrollAxis<T> = Arc<dyn Fn(&T) -> bool + Send + Sync>;
type DesktopItemSnap<T> = Arc<dyn Fn(&T) -> Option<(f64, f64)> + Send + Sync>;
type DesktopSnapStop<T> = Arc<dyn Fn(&T) -> bool + Send + Sync>;
type DesktopScrollEnabled<T> = Arc<dyn Fn(&T) -> bool + Send + Sync>;
type DesktopInputFocused<T> = Arc<dyn Fn(&T) -> bool + Send + Sync>;
type DesktopSetInputFocus<T> = Arc<dyn Fn(&mut T, bool) + Send + Sync>;
type DesktopInputHandler<T> = Arc<dyn Fn(&mut T, &DesktopTextInputEvent) + Send + Sync>;
type DesktopSelectedText<T> = Arc<dyn Fn(&T) -> Option<String> + Send + Sync>;

struct DesktopTextInputBinding<T> {
    focused: DesktopInputFocused<T>,
    set_focus: DesktopSetInputFocus<T>,
    input: DesktopInputHandler<T>,
    selected_text: DesktopSelectedText<T>,
}

impl<T> Clone for DesktopTextInputBinding<T> {
    fn clone(&self) -> Self {
        Self {
            focused: Arc::clone(&self.focused),
            set_focus: Arc::clone(&self.set_focus),
            input: Arc::clone(&self.input),
            selected_text: Arc::clone(&self.selected_text),
        }
    }
}

struct DesktopPropBinding<T> {
    set: DesktopPropSetter<T>,
    clear: DesktopPropClearer<T>,
}

impl<T> Clone for DesktopPropBinding<T> {
    fn clone(&self) -> Self {
        Self {
            set: Arc::clone(&self.set),
            clear: Arc::clone(&self.clear),
        }
    }
}

/// The `View { Prop / Events / Command }` portion of a Desktop declaration.
pub struct DesktopViewDefinition<T> {
    name: String,
    create: Arc<dyn Fn(DesktopEventEmitter) -> T + Send + Sync>,
    properties: HashMap<String, DesktopPropBinding<T>>,
    events: HashSet<String>,
    commands: HashMap<String, DesktopCommandHandler<T>>,
    text_style: Option<DesktopTextStyleUpdater<T>>,
    pub(super) measurement: Option<DesktopMeasurementHandler>,
    rasterizer: Option<DesktopRasterizer<T>>,
    plain_text: bool,
    scroll_content: bool,
    scroll_horizontal: DesktopScrollAxis<T>,
    item_snap: DesktopItemSnap<T>,
    snap_stop_always: DesktopSnapStop<T>,
    scroll_enabled: DesktopScrollEnabled<T>,
    text_input: Option<DesktopTextInputBinding<T>>,
}

impl<T> DesktopViewDefinition<T>
where
    T: 'static,
{
    /// Declares how a Desktop content object is created for each mounted node.
    pub fn new(
        name: impl Into<String>,
        create: impl Fn(DesktopEventEmitter) -> T + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            create: Arc::new(create),
            properties: HashMap::new(),
            events: HashSet::new(),
            commands: HashMap::new(),
            text_style: None,
            measurement: None,
            rasterizer: None,
            plain_text: false,
            scroll_content: false,
            scroll_horizontal: Arc::new(|_| false),
            item_snap: Arc::new(|_| None),
            snap_stop_always: Arc::new(|_| false),
            scroll_enabled: Arc::new(|_| true),
            text_input: None,
        }
    }

    /// Declares that this Host implementation consumes normalized plain-text
    /// content through the common Desktop text renderer.
    pub fn plain_text(mut self) -> Self {
        self.plain_text = true;
        self
    }

    /// Declares that the Host object owns transient vertical scroll state.
    pub fn scroll_container(mut self) -> Self {
        self.scroll_content = true;
        self
    }

    pub(crate) fn scroll_behavior(
        mut self,
        horizontal: impl Fn(&T) -> bool + Send + Sync + 'static,
        item_snap: impl Fn(&T) -> Option<(f64, f64)> + Send + Sync + 'static,
        snap_stop_always: impl Fn(&T) -> bool + Send + Sync + 'static,
        scroll_enabled: impl Fn(&T) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.scroll_content = true;
        self.scroll_horizontal = Arc::new(horizontal);
        self.item_snap = Arc::new(item_snap);
        self.snap_stop_always = Arc::new(snap_stop_always);
        self.scroll_enabled = Arc::new(scroll_enabled);
        self
    }

    /// Declares one property by its stable Host name.
    pub fn prop(
        mut self,
        property: impl Into<String>,
        set: impl Fn(&mut T, &WhiskerValue) + Send + Sync + 'static,
        clear: impl Fn(&mut T) + Send + Sync + 'static,
    ) -> Self {
        let property = property.into();
        assert!(
            !property.trim().is_empty(),
            "Desktop property name is empty"
        );
        assert!(
            self.properties
                .insert(
                    property.clone(),
                    DesktopPropBinding {
                        set: Arc::new(set),
                        clear: Arc::new(clear),
                    },
                )
                .is_none(),
            "duplicate Desktop property binding for {property}"
        );
        self
    }

    /// Declares one event by its stable Host name.
    pub fn event(mut self, event: impl Into<String>) -> Self {
        let event = event.into();
        assert!(!event.trim().is_empty(), "Desktop event name is empty");
        assert!(
            self.events.insert(event.clone()),
            "duplicate Desktop event binding for {event}"
        );
        self
    }

    /// Declares one command by its stable Host name.
    pub fn command(
        mut self,
        command: impl Into<String>,
        handler: impl Fn(&mut T, &WhiskerValue) + Send + Sync + 'static,
    ) -> Self {
        let command = command.into();
        assert!(!command.trim().is_empty(), "Desktop command name is empty");
        assert!(
            self.commands
                .insert(command.clone(), Arc::new(handler))
                .is_none(),
            "duplicate Desktop command binding for {command}"
        );
        self
    }

    /// Declares that this native content object consumes resolved inherited
    /// text style independently from plain-text children.
    pub fn text_style(
        mut self,
        update: impl Fn(&mut T, &WhiskerTextStyle) + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.text_style.replace(Arc::new(update)).is_none(),
            "duplicate Desktop TextStyle binding for {}",
            self.name
        );
        self
    }

    /// Declares a module-owned editable-text object. The package retains the
    /// value, selection, and composition model; the shared Desktop Host only
    /// routes OS focus, keyboard, clipboard, and IME messages.
    pub fn text_input(
        mut self,
        focused: impl Fn(&T) -> bool + Send + Sync + 'static,
        set_focus: impl Fn(&mut T, bool) + Send + Sync + 'static,
        input: impl Fn(&mut T, &DesktopTextInputEvent) + Send + Sync + 'static,
        selected_text: impl Fn(&T) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.text_input.is_none(),
            "duplicate Desktop text-input binding for {}",
            self.name
        );
        self.text_input = Some(DesktopTextInputBinding {
            focused: Arc::new(focused),
            set_focus: Arc::new(set_focus),
            input: Arc::new(input),
            selected_text: Arc::new(selected_text),
        });
        self
    }

    /// Supplies synchronous Host intrinsic measurement for Custom or
    /// ReplacedContent schemas. `None` means unsupported for this request.
    pub fn measurement(
        mut self,
        measure: impl Fn(&WhiskerMeasureRequest) -> Option<WhiskerMeasuredSize> + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.measurement.replace(Arc::new(measure)).is_none(),
            "duplicate Desktop Measurement binding for {}",
            self.name
        );
        self
    }

    /// Declares module-owned raster content painted inside the element's
    /// computed content box.
    pub fn raster(
        mut self,
        rasterize: impl Fn(&T, u32, u32) -> Option<DesktopRaster> + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.rasterizer
                .replace(Arc::new(move |state, width, height, _scale| {
                    rasterize(state, width, height)
                }))
                .is_none(),
            "duplicate Desktop raster binding for {}",
            self.name,
        );
        self
    }

    /// Declares scale-aware module-owned raster content.
    pub fn raster_scaled(
        mut self,
        rasterize: impl Fn(&T, u32, u32, f32) -> Option<DesktopRaster> + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.rasterizer.replace(Arc::new(rasterize)).is_none(),
            "duplicate Desktop raster binding for {}",
            self.name,
        );
        self
    }

    pub(super) fn bind(
        &self,
        registration: &ElementRegistration,
    ) -> Result<NativeConstructor, String> {
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
            ElementMeasurement::ReplacedContent | ElementMeasurement::Custom
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
            .map(|schema| (schema.command, Arc::clone(&self.commands[&schema.name])))
            .collect();
        let definition = Arc::new(BoundDesktopViewDefinition {
            create: Arc::clone(&self.create),
            properties,
            commands,
            text_style: self.text_style.clone(),
            rasterizer: self.rasterizer.clone(),
            scroll_content: self.scroll_content,
            scroll_horizontal: Arc::clone(&self.scroll_horizontal),
            item_snap: Arc::clone(&self.item_snap),
            snap_stop_always: Arc::clone(&self.snap_stop_always),
            scroll_enabled: Arc::clone(&self.scroll_enabled),
            text_input: self.text_input.clone(),
        });
        Ok(Arc::new(move |events| {
            Box::new(DeclaredDesktopElement {
                state: (definition.create)(events),
                definition: definition.clone(),
            })
        }))
    }
}

impl<T> DesktopViewImplementation for DesktopViewDefinition<T>
where
    T: Send + Sync + 'static,
{
    fn into_desktop_factory(self) -> DesktopElementFactory {
        let name = self.name.clone();
        let plain_text = self.plain_text;
        DesktopElementFactory::declared(name, self, plain_text)
    }
}

struct BoundDesktopViewDefinition<T> {
    create: Arc<dyn Fn(DesktopEventEmitter) -> T + Send + Sync>,
    properties: HashMap<PropertyId, DesktopPropBinding<T>>,
    commands: HashMap<CommandId, DesktopCommandHandler<T>>,
    text_style: Option<DesktopTextStyleUpdater<T>>,
    rasterizer: Option<DesktopRasterizer<T>>,
    scroll_content: bool,
    scroll_horizontal: DesktopScrollAxis<T>,
    item_snap: DesktopItemSnap<T>,
    snap_stop_always: DesktopSnapStop<T>,
    scroll_enabled: DesktopScrollEnabled<T>,
    text_input: Option<DesktopTextInputBinding<T>>,
}

struct DeclaredDesktopElement<T> {
    state: T,
    definition: Arc<BoundDesktopViewDefinition<T>>,
}

impl<T> fmt::Debug for DeclaredDesktopElement<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeclaredDesktopElement(..)")
    }
}

impl<T> DesktopNativeElement for DeclaredDesktopElement<T>
where
    T: 'static,
{
    fn set_property(&mut self, property: PropertyId, value: &WhiskerValue) {
        let binding = self
            .definition
            .properties
            .get(&property)
            .expect("Desktop Host validates property IDs");
        (binding.set)(&mut self.state, value);
    }

    fn clear_property(&mut self, property: PropertyId) {
        let binding = self
            .definition
            .properties
            .get(&property)
            .expect("Desktop Host validates property IDs");
        (binding.clear)(&mut self.state);
    }

    fn invoke_command(&mut self, command: CommandId, arguments: &WhiskerValue) {
        let handler = self
            .definition
            .commands
            .get(&command)
            .expect("Desktop Host validates command IDs");
        handler(&mut self.state, arguments);
    }

    fn set_text_style(&mut self, style: &WhiskerTextStyle) {
        if let Some(update) = &self.definition.text_style {
            update(&mut self.state, style);
        }
    }

    fn rasterize(&self, width: u32, height: u32) -> Option<DesktopRaster> {
        self.rasterize_scaled(width, height, 1.0)
    }

    fn rasterize_scaled(&self, width: u32, height: u32, scale: f32) -> Option<DesktopRaster> {
        self.definition
            .rasterizer
            .as_ref()
            .and_then(|rasterize| rasterize(&self.state, width, height, scale))
    }

    fn has_raster_content(&self) -> bool {
        self.definition.rasterizer.is_some()
    }

    fn accepts_text_input(&self) -> bool {
        self.definition.text_input.is_some()
    }

    fn text_input_focused(&self) -> bool {
        self.definition
            .text_input
            .as_ref()
            .is_some_and(|binding| (binding.focused)(&self.state))
    }

    fn set_text_input_focus(&mut self, focused: bool) {
        if let Some(binding) = &self.definition.text_input {
            (binding.set_focus)(&mut self.state, focused);
        }
    }

    fn handle_text_input(&mut self, event: &DesktopTextInputEvent) {
        if let Some(binding) = &self.definition.text_input {
            (binding.input)(&mut self.state, event);
        }
    }

    fn selected_text(&self) -> Option<String> {
        self.definition
            .text_input
            .as_ref()
            .and_then(|binding| (binding.selected_text)(&self.state))
    }

    fn is_scroll_container(&self) -> bool {
        self.definition.scroll_content
    }

    fn scroll_horizontal(&self) -> bool {
        (self.definition.scroll_horizontal)(&self.state)
    }

    fn item_snap(&self) -> Option<(f64, f64)> {
        (self.definition.item_snap)(&self.state)
    }

    fn snap_stop_always(&self) -> bool {
        (self.definition.snap_stop_always)(&self.state)
    }

    fn scroll_enabled(&self) -> bool {
        (self.definition.scroll_enabled)(&self.state)
    }
}
