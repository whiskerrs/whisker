//! Desktop binding for negotiated element schemas.

use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use whisker::WhiskerModule;
use whisker_protocol::{
    ChildPolicy, CommandId, ElementMeasurement, ElementRegistration, ElementRegistrationError,
    ElementTypeId, EventId, NodeId, PropertyId, TextContent, WhiskerValue,
};

/// Rust-native counterpart of the Swift/Kotlin `ModuleDefinition` DSL.
#[derive(Clone, Debug, Default)]
pub struct DesktopModuleDefinition {
    factories: Vec<DesktopElementFactory>,
}

impl DesktopModuleDefinition {
    /// Starts an empty Desktop module declaration.
    pub fn new() -> Self {
        Self::default()
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
type DesktopCommandHandler<T> =
    Arc<dyn Fn(&mut T, &WhiskerValue) -> Option<DesktopNativeEvent> + Send + Sync>;
type DesktopRasterizer<T> = Arc<dyn Fn(&T, u32, u32) -> Option<DesktopRaster> + Send + Sync>;

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
    create: Arc<dyn Fn() -> T + Send + Sync>,
    properties: HashMap<String, DesktopPropBinding<T>>,
    events: HashSet<String>,
    commands: HashMap<String, DesktopCommandHandler<T>>,
    rasterizer: Option<DesktopRasterizer<T>>,
    plain_text: bool,
}

impl<T> DesktopViewDefinition<T>
where
    T: 'static,
{
    /// Declares how a Desktop content object is created for each mounted node.
    pub fn new(name: impl Into<String>, create: impl Fn() -> T + Send + Sync + 'static) -> Self {
        Self {
            name: name.into(),
            create: Arc::new(create),
            properties: HashMap::new(),
            events: HashSet::new(),
            commands: HashMap::new(),
            rasterizer: None,
            plain_text: false,
        }
    }

    /// Declares that this Host implementation consumes normalized plain-text
    /// content through the common Desktop text renderer.
    pub fn plain_text(mut self) -> Self {
        self.plain_text = true;
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
        handler: impl Fn(&mut T, &WhiskerValue) -> Option<DesktopNativeEvent> + Send + Sync + 'static,
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

    /// Declares module-owned raster content painted inside the element's
    /// computed content box.
    pub fn raster(
        mut self,
        rasterize: impl Fn(&T, u32, u32) -> Option<DesktopRaster> + Send + Sync + 'static,
    ) -> Self {
        assert!(
            self.rasterizer.replace(Arc::new(rasterize)).is_none(),
            "duplicate Desktop raster binding for {}",
            self.name,
        );
        self
    }

    fn bind(&self, registration: &ElementRegistration) -> Result<NativeConstructor, String> {
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
            .map(|schema| (schema.command, Arc::clone(&self.commands[&schema.name])))
            .collect();
        let definition = Arc::new(BoundDesktopViewDefinition {
            create: Arc::clone(&self.create),
            properties,
            commands,
            rasterizer: self.rasterizer.clone(),
        });
        Ok(Arc::new(move || {
            Box::new(DeclaredDesktopElement {
                state: (definition.create)(),
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
    create: Arc<dyn Fn() -> T + Send + Sync>,
    properties: HashMap<PropertyId, DesktopPropBinding<T>>,
    commands: HashMap<CommandId, DesktopCommandHandler<T>>,
    rasterizer: Option<DesktopRasterizer<T>>,
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

    fn invoke_command(
        &mut self,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Option<DesktopNativeEvent> {
        let handler = self
            .definition
            .commands
            .get(&command)
            .expect("Desktop Host validates command IDs");
        handler(&mut self.state, arguments)
    }

    fn rasterize(&self, width: u32, height: u32) -> Option<DesktopRaster> {
        self.definition
            .rasterizer
            .as_ref()
            .and_then(|rasterize| rasterize(&self.state, width, height))
    }

    fn has_raster_content(&self) -> bool {
        self.definition.rasterizer.is_some()
    }
}

/// Target-specific Desktop factory embedded for one element module.
///
/// The element name joins this Host definition to the Rust-side
/// [`whisker::ElementSchema`]. Constructors intentionally expose only
/// the content factories implemented by the current Desktop Host.
#[derive(Clone)]
pub struct DesktopElementFactory {
    name: String,
    kind: DesktopElementFactoryKind,
    plain_text: bool,
}

impl DesktopElementFactory {
    /// Creates a common-presentation-only element factory.
    pub fn presentation(name: impl Into<String>) -> Self {
        Self::new(name, DesktopElementFactoryKind::Presentation, false)
    }

    /// Creates a native Desktop text-content factory.
    pub fn text(name: impl Into<String>) -> Self {
        Self::new(name, DesktopElementFactoryKind::Text, true)
    }

    /// Creates a Desktop scroll-container factory.
    pub fn scroll_container(name: impl Into<String>) -> Self {
        Self::new(name, DesktopElementFactoryKind::ScrollContainer, false)
    }

    /// Creates an element whose content and commands are implemented by a
    /// module-owned native Desktop object.
    pub fn native<F>(name: impl Into<String>, create: F) -> Self
    where
        F: Fn() -> Box<dyn DesktopNativeElement> + Send + Sync + 'static,
    {
        Self::new(
            name,
            DesktopElementFactoryKind::Native(Arc::new(create)),
            false,
        )
    }

    fn declared<T>(
        name: impl Into<String>,
        definition: DesktopViewDefinition<T>,
        plain_text: bool,
    ) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::new(
            name,
            DesktopElementFactoryKind::Declared(Arc::new(definition)),
            plain_text,
        )
    }

    fn new(name: impl Into<String>, kind: DesktopElementFactoryKind, plain_text: bool) -> Self {
        Self {
            name: name.into(),
            kind,
            plain_text,
        }
    }

    fn kind_name(&self) -> &'static str {
        match &self.kind {
            DesktopElementFactoryKind::Presentation => "presentation",
            DesktopElementFactoryKind::Text => "text",
            DesktopElementFactoryKind::ScrollContainer => "scroll-container",
            DesktopElementFactoryKind::Native(_) => "native",
            DesktopElementFactoryKind::Declared(_) => "declared-native",
        }
    }

    fn bind(&self, registration: &ElementRegistration) -> Result<Self, DesktopElementError> {
        if registration.child_policy.accepts_plain_text() != self.plain_text {
            return Err(DesktopElementError::FactoryContractMismatch {
                name: registration.name.clone(),
                reason: format!(
                    "plain-text policy differs: Host={}, Rust={:?}",
                    self.plain_text, registration.child_policy
                ),
            });
        }
        let kind = match &self.kind {
            DesktopElementFactoryKind::Declared(definition) => {
                DesktopElementFactoryKind::Native(definition.bind(registration).map_err(
                    |reason| DesktopElementError::FactoryContractMismatch {
                        name: registration.name.clone(),
                        reason,
                    },
                )?)
            }
            other => other.clone(),
        };
        Ok(Self {
            name: self.name.clone(),
            kind,
            plain_text: self.plain_text,
        })
    }

    fn create(&self) -> DesktopElementContent {
        match &self.kind {
            DesktopElementFactoryKind::Presentation => DesktopElementContent::Empty,
            DesktopElementFactoryKind::Text => DesktopElementContent::Text(None),
            DesktopElementFactoryKind::ScrollContainer => DesktopElementContent::ScrollContainer,
            DesktopElementFactoryKind::Native(create) => DesktopElementContent::Native {
                implementation: create(),
                text: None,
                plain_text: self.plain_text,
            },
            DesktopElementFactoryKind::Declared(_) => {
                unreachable!("Desktop declared factory was not bound at bootstrap")
            }
        }
    }
}

impl fmt::Debug for DesktopElementFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DesktopElementFactory")
            .field("name", &self.name)
            .field("kind", &self.kind_name())
            .finish()
    }
}

type NativeConstructor = Arc<dyn Fn() -> Box<dyn DesktopNativeElement> + Send + Sync>;

trait DesktopDeclaredFactory: Send + Sync {
    fn bind(&self, registration: &ElementRegistration) -> Result<NativeConstructor, String>;
}

impl<T> DesktopDeclaredFactory for DesktopViewDefinition<T>
where
    T: Send + Sync + 'static,
{
    fn bind(&self, registration: &ElementRegistration) -> Result<NativeConstructor, String> {
        DesktopViewDefinition::bind(self, registration)
    }
}

#[derive(Clone)]
enum DesktopElementFactoryKind {
    Presentation,
    Text,
    ScrollContainer,
    Native(NativeConstructor),
    Declared(Arc<dyn DesktopDeclaredFactory>),
}

/// Event emitted by a module-owned Desktop native element.
#[derive(Clone, Debug, PartialEq)]
pub struct DesktopNativeEvent {
    /// Stable event name declared by the Host module.
    pub event: String,
    /// Typed event detail routed to the Rust listener.
    pub detail: WhiskerValue,
}

/// RGBA8 content produced by a module-owned Desktop element.
///
/// The module keeps vector decoding or native drawing dependencies on its own
/// side of the Host boundary. The shared Desktop renderer only uploads this
/// platform-neutral pixel buffer and composites it with common Whisker layout,
/// clipping, transforms, and opacity.
#[derive(Clone, Debug)]
pub struct DesktopRaster {
    generation: u64,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

impl DesktopRaster {
    /// Creates a validated straight-alpha RGBA8 raster.
    pub fn new(
        generation: u64,
        width: u32,
        height: u32,
        pixels: impl Into<Arc<[u8]>>,
    ) -> Result<Self, DesktopRasterError> {
        let pixels = pixels.into();
        let expected = width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            .map(|count| count as usize)
            .ok_or(DesktopRasterError::DimensionsOverflow)?;
        if width == 0 || height == 0 {
            return Err(DesktopRasterError::EmptyDimensions);
        }
        if pixels.len() != expected {
            return Err(DesktopRasterError::ByteLength {
                actual: pixels.len(),
                expected,
            });
        }
        Ok(Self {
            generation,
            width,
            height,
            pixels,
        })
    }

    /// Module-defined generation used by the GPU upload cache.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Physical pixel width.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Physical pixel height.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Straight-alpha RGBA8 bytes in row-major order.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// Invalid module-owned Desktop raster data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopRasterError {
    /// Width or height was zero.
    EmptyDimensions,
    /// Computing `width * height * 4` overflowed.
    DimensionsOverflow,
    /// The RGBA8 payload did not match its dimensions.
    ByteLength {
        /// Received byte count.
        actual: usize,
        /// Required byte count.
        expected: usize,
    },
}

impl fmt::Display for DesktopRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDimensions => formatter.write_str("Desktop raster dimensions are empty"),
            Self::DimensionsOverflow => formatter.write_str("Desktop raster dimensions overflow"),
            Self::ByteLength { actual, expected } => write!(
                formatter,
                "Desktop raster has {actual} bytes, expected {expected}",
            ),
        }
    }
}

impl Error for DesktopRasterError {}

/// Target definition implemented beside an external element's Rust schema.
///
/// The schema remains the source of truth for valid IDs and value shapes; the
/// Desktop Host validates each operation before dispatching it here.
pub trait DesktopNativeElement: fmt::Debug + 'static {
    /// Applies one negotiated property.
    fn set_property(&mut self, property: PropertyId, value: &WhiskerValue);

    /// Restores one negotiated property to its implementation default.
    fn clear_property(&mut self, property: PropertyId);

    /// Executes one negotiated command and optionally emits an event.
    fn invoke_command(
        &mut self,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Option<DesktopNativeEvent>;

    /// Produces element-owned pixels for the requested physical bounds.
    ///
    /// Implementations should cache by `(generation, width, height)` and may
    /// return `None` for empty or temporarily unavailable content. The default
    /// keeps state-only custom elements allocation-free.
    fn rasterize(&self, _width: u32, _height: u32) -> Option<DesktopRaster> {
        None
    }

    /// Whether this element contributes raster content at all.
    fn has_raster_content(&self) -> bool {
        false
    }
}

/// Built-in element implementations contributed by the Desktop platform.
pub struct BuiltInElementModule;

#[WhiskerModule]
impl WhiskerModule for BuiltInElementModule {
    type Definition = DesktopModuleDefinition;

    fn definition() -> Self::Definition {
        DesktopModuleDefinition::new()
            .view(DesktopViewDefinition::new("whisker.ui/View", || ()))
            .view(DesktopViewDefinition::new("whisker.ui/Text", || ()).plain_text())
            .view(DesktopViewDefinition::new("whisker.ui/ScrollView", || ()))
    }
}

#[cfg(test)]
pub(crate) fn built_in_element_factories() -> Vec<DesktopElementFactory> {
    BuiltInElementModule::definition().into_factories()
}

/// Element-specific state retained beside common Desktop presentation.
#[derive(Debug)]
pub(crate) enum DesktopElementContent {
    Empty,
    Text(Option<TextContent>),
    ScrollContainer,
    Native {
        implementation: Box<dyn DesktopNativeElement>,
        text: Option<TextContent>,
        plain_text: bool,
    },
}

impl DesktopElementContent {
    pub(crate) fn text(&self) -> Option<&TextContent> {
        match self {
            Self::Text(content) => content.as_ref(),
            Self::Native { text, .. } => text.as_ref(),
            Self::Empty | Self::ScrollContainer => None,
        }
    }

    pub(crate) fn rasterizer(&self) -> Option<&dyn DesktopNativeElement> {
        match self {
            Self::Native { implementation, .. } if implementation.has_raster_content() => {
                Some(implementation.as_ref())
            }
            Self::Native { .. } => None,
            Self::Empty | Self::Text(_) | Self::ScrollContainer => None,
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
            Self::Native {
                text,
                plain_text: true,
                ..
            } => {
                *text = Some(content);
                Ok(())
            }
            Self::Empty | Self::ScrollContainer | Self::Native { .. } => {
                Err(DesktopElementError::UnexpectedText { node })
            }
        }
    }

    pub(crate) fn set_property(
        &mut self,
        node: NodeId,
        property: PropertyId,
        value: &WhiskerValue,
    ) -> Result<(), DesktopElementError> {
        match self {
            Self::Native { implementation, .. } => {
                implementation.set_property(property, value);
                Ok(())
            }
            Self::Empty | Self::Text(_) | Self::ScrollContainer => {
                Err(DesktopElementError::UnsupportedProperty { node, property })
            }
        }
    }

    pub(crate) fn clear_property(
        &mut self,
        node: NodeId,
        property: PropertyId,
    ) -> Result<(), DesktopElementError> {
        match self {
            Self::Native { implementation, .. } => {
                implementation.clear_property(property);
                Ok(())
            }
            Self::Empty | Self::Text(_) | Self::ScrollContainer => {
                Err(DesktopElementError::UnsupportedProperty { node, property })
            }
        }
    }

    pub(crate) fn invoke_command(
        &mut self,
        node: NodeId,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Result<Option<DesktopNativeEvent>, DesktopElementError> {
        match self {
            Self::Native { implementation, .. } => {
                Ok(implementation.invoke_command(command, arguments))
            }
            Self::Empty | Self::Text(_) | Self::ScrollContainer => {
                Err(DesktopElementError::UnsupportedCommand { node, command })
            }
        }
    }
}

#[derive(Clone, Debug)]
struct DesktopElementBinding {
    registration: ElementRegistration,
    factory: DesktopElementFactory,
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
        let mut elements_by_name = HashMap::with_capacity(registrations.len());
        let mut factories_by_name = HashMap::with_capacity(factories.len());
        for factory in factories {
            if factories_by_name
                .insert(factory.name.clone(), factory.clone())
                .is_some()
            {
                return Err(DesktopElementError::DuplicateFactory {
                    name: factory.name.clone(),
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
            let identity = registration.name.clone();
            if elements_by_name
                .insert(identity, registration.element_type)
                .is_some()
            {
                return Err(DesktopElementError::DuplicateElementName {
                    element_type: registration.element_type,
                });
            }
            let factory = factories_by_name
                .remove(&registration.name)
                .ok_or_else(|| DesktopElementError::MissingFactory {
                    name: registration.name.clone(),
                })?
                .bind(registration)?;
            bindings.insert(
                registration.element_type,
                DesktopElementBinding {
                    registration: registration.clone(),
                    factory,
                    measurement: registration.measurement,
                },
            );
        }
        if let Some(name) = factories_by_name.into_keys().next() {
            return Err(DesktopElementError::UnknownFactory { name });
        }
        Ok(Self { bindings })
    }

    pub(crate) fn create(
        &self,
        element_type: ElementTypeId,
    ) -> Result<DesktopElementContent, DesktopElementError> {
        Ok(self.binding(element_type)?.factory.create())
    }

    pub(crate) fn child_policy(
        &self,
        element_type: ElementTypeId,
    ) -> Result<ChildPolicy, DesktopElementError> {
        Ok(self.binding(element_type)?.registration.child_policy)
    }

    pub(crate) fn measurement(
        &self,
        element_type: ElementTypeId,
    ) -> Result<ElementMeasurement, DesktopElementError> {
        Ok(self.binding(element_type)?.measurement)
    }

    pub(crate) fn validate_property(
        &self,
        element_type: ElementTypeId,
        node: NodeId,
        property: PropertyId,
        value: Option<&WhiskerValue>,
    ) -> Result<(), DesktopElementError> {
        let registration = &self.binding(element_type)?.registration;
        let schema = registration
            .property(property)
            .ok_or(DesktopElementError::UnsupportedProperty { node, property })?;
        if value.is_some_and(|value| !schema.value.accepts(value)) {
            return Err(DesktopElementError::InvalidPropertyValue {
                node,
                property,
                expected: schema.value,
            });
        }
        Ok(())
    }

    pub(crate) fn validate_command(
        &self,
        element_type: ElementTypeId,
        node: NodeId,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Result<(), DesktopElementError> {
        let registration = &self.binding(element_type)?.registration;
        let schema = registration
            .command(command)
            .ok_or(DesktopElementError::UnsupportedCommand { node, command })?;
        if !schema.arguments.accepts(arguments) {
            return Err(DesktopElementError::InvalidCommandArguments {
                node,
                command,
                expected: schema.arguments,
            });
        }
        Ok(())
    }

    pub(crate) fn event(
        &self,
        element_type: ElementTypeId,
        node: NodeId,
        event: &str,
        detail: &WhiskerValue,
    ) -> Result<(String, u64), DesktopElementError> {
        let registration = &self.binding(element_type)?.registration;
        let schema = registration.event_named(event).ok_or_else(|| {
            DesktopElementError::UnsupportedEvent {
                node,
                event: event.to_string(),
            }
        })?;
        if !schema.accepts_detail(detail) {
            return Err(DesktopElementError::InvalidEventDetail {
                node,
                event: schema.event,
                expected: schema.detail,
            });
        }
        Ok((
            schema.name.clone(),
            schema
                .mask()
                .expect("registration validation checked event ID"),
        ))
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
    DuplicateElementName {
        element_type: ElementTypeId,
    },
    DuplicateFactory {
        name: String,
    },
    MissingFactory {
        name: String,
    },
    UnknownFactory {
        name: String,
    },
    FactoryContractMismatch {
        name: String,
        reason: String,
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
        property: PropertyId,
    },
    InvalidPropertyValue {
        node: NodeId,
        property: PropertyId,
        expected: whisker_protocol::ElementValueKind,
    },
    UnsupportedCommand {
        node: NodeId,
        command: CommandId,
    },
    InvalidCommandArguments {
        node: NodeId,
        command: CommandId,
        expected: whisker_protocol::ElementValueKind,
    },
    UnsupportedEvent {
        node: NodeId,
        event: String,
    },
    InvalidEventDetail {
        node: NodeId,
        event: EventId,
        expected: Option<whisker_protocol::ElementValueKind>,
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
    use whisker::{ElementRegistry, SurfaceRuntime, standard_element_registrations};
    use whisker_protocol::{
        ElementMeasurement, ElementPropertySchema, ElementSchema, ElementValueKind, SurfaceId,
    };
    use whisker_style::StyleEnvironment;

    use super::*;

    #[test]
    fn desktop_raster_validates_dimensions_and_rgba_length() {
        assert_eq!(
            DesktopRaster::new(1, 0, 1, Vec::<u8>::new()).unwrap_err(),
            DesktopRasterError::EmptyDimensions,
        );
        assert_eq!(
            DesktopRaster::new(1, 2, 2, vec![0; 15]).unwrap_err(),
            DesktopRasterError::ByteLength {
                actual: 15,
                expected: 16,
            },
        );
        let raster = DesktopRaster::new(7, 2, 2, vec![255; 16]).unwrap();
        assert_eq!(raster.generation(), 7);
        assert_eq!((raster.width(), raster.height()), (2, 2));
        assert_eq!(raster.pixels(), &[255; 16]);
    }

    #[test]
    fn built_in_module_binds_view_text_and_scroll_through_one_registry() {
        let definition = BuiltInElementModule::definition();
        assert_eq!(definition.factories().len(), 3);
        let registrations = standard_element_registrations();
        let factories = built_in_element_factories();
        let registry = DesktopElementRegistry::bind(&registrations, &factories).unwrap();
        for registration in &registrations {
            let content = registry.create(registration.element_type).unwrap();
            assert_eq!(
                matches!(
                    content,
                    DesktopElementContent::Text(_)
                        | DesktopElementContent::Native {
                            plain_text: true,
                            ..
                        }
                ),
                registration.name == whisker::TEXT_ELEMENT_NAME
            );
            assert_eq!(
                registry.child_policy(registration.element_type).unwrap(),
                registration.child_policy
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
            registration.name == whisker::TEXT_ELEMENT_NAME
                && registration.measurement == ElementMeasurement::Text
        }));
    }

    #[test]
    fn duplicate_and_missing_registrations_fail_before_a_frame() {
        let mut registrations = standard_element_registrations();
        registrations.push(registrations[0].clone());
        assert!(matches!(
            DesktopElementRegistry::bind(&registrations, &built_in_element_factories()),
            Err(DesktopElementError::DuplicateElementType { .. })
        ));

        assert!(matches!(
            DesktopElementRegistry::bind(&standard_element_registrations(), &[]),
            Err(DesktopElementError::MissingFactory { .. })
        ));
    }

    #[test]
    fn module_binding_binds_by_versionless_name() {
        let schema = ElementSchema {
            name: "whisker.test/Badge".into(),
            child_policy: ChildPolicy::Elements,
            measurement: ElementMeasurement::None,
            properties: Vec::new(),
            events: Vec::new(),
            commands: Vec::new(),
        };
        let elements = ElementRegistry::standard_builder()
            .register_provider(whisker::ElementProviderMetadata::named(schema))
            .build()
            .unwrap();
        let badge = elements
            .registration_for_name("whisker.test/Badge")
            .unwrap();
        let mut factories = built_in_element_factories();
        factories.push(DesktopElementFactory::presentation("whisker.test/Badge"));
        let desktop = DesktopElementRegistry::bind(elements.registrations(), &factories).unwrap();

        assert!(matches!(
            desktop.create(badge.element_type),
            Ok(DesktopElementContent::Empty)
        ));
        assert_eq!(badge.name, "whisker.test/Badge");
    }

    #[test]
    fn declared_host_members_must_match_the_rust_schema_at_bootstrap() {
        let registration = ElementRegistration {
            element_type: ElementTypeId::new(20).unwrap(),
            name: "whisker.test/Toggle".into(),
            child_policy: ChildPolicy::None,
            measurement: ElementMeasurement::None,
            properties: vec![ElementPropertySchema {
                property: PropertyId::new(1).unwrap(),
                name: "checked".into(),
                value: ElementValueKind::Bool,
            }],
            events: Vec::new(),
            commands: Vec::new(),
        };
        let definition = DesktopViewDefinition::new("whisker.test/Toggle", || ()).prop(
            "misspelled",
            |_, _| {},
            |_| {},
        );
        let factory = definition.into_desktop_factory();

        assert!(matches!(
            DesktopElementRegistry::bind(&[registration], &[factory]),
            Err(DesktopElementError::FactoryContractMismatch { name, .. })
                if name == "whisker.test/Toggle"
        ));
    }
}
