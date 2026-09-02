//! Desktop binding for negotiated element schemas.

use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use whisker::WhiskerModule;
use whisker::runtime::module::{ModuleEventEmitter, ModulePromise, RustModuleDefinition};
use whisker_protocol::{
    ChildPolicy, CommandId, ElementMeasurement, ElementRegistration, ElementRegistrationError,
    ElementTypeId, EventId, MeasurementMetrics, MeasurementRequest, MeasurementResponse, NodeId,
    PropertyId, TextContent, UnsupportedMeasurementReason, WhiskerValue,
};

use crate::{WhiskerMeasureRequest, WhiskerMeasuredSize, WhiskerTextStyle};

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
    measurer: Option<DesktopMeasurementHandler>,
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
        F: Fn(DesktopEventEmitter) -> Box<dyn DesktopNativeElement> + Send + Sync + 'static,
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
            measurer: None,
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
        let measurer = match &self.kind {
            DesktopElementFactoryKind::Declared(definition) => definition.measurer(),
            _ => self.measurer.clone(),
        };
        let needs_host_measurement = matches!(
            registration.measurement,
            ElementMeasurement::ReplacedContent | ElementMeasurement::Custom
        );
        if needs_host_measurement != measurer.is_some() {
            return Err(DesktopElementError::FactoryContractMismatch {
                name: registration.name.clone(),
                reason: format!(
                    "measurement capability differs: Host={}, Rust={:?}",
                    measurer.is_some(),
                    registration.measurement
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
            measurer,
        })
    }

    fn create(&self, events: DesktopEventEmitter) -> DesktopElementContent {
        match &self.kind {
            DesktopElementFactoryKind::Presentation => DesktopElementContent::Empty,
            DesktopElementFactoryKind::Text => DesktopElementContent::Text(None),
            DesktopElementFactoryKind::ScrollContainer => DesktopElementContent::ScrollContainer,
            DesktopElementFactoryKind::Native(create) => DesktopElementContent::Native {
                implementation: create(events),
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

type NativeConstructor =
    Arc<dyn Fn(DesktopEventEmitter) -> Box<dyn DesktopNativeElement> + Send + Sync>;

trait DesktopDeclaredFactory: Send + Sync {
    fn bind(&self, registration: &ElementRegistration) -> Result<NativeConstructor, String>;

    fn measurer(&self) -> Option<DesktopMeasurementHandler>;
}

impl<T> DesktopDeclaredFactory for DesktopViewDefinition<T>
where
    T: Send + Sync + 'static,
{
    fn bind(&self, registration: &ElementRegistration) -> Result<NativeConstructor, String> {
        DesktopViewDefinition::bind(self, registration)
    }

    fn measurer(&self) -> Option<DesktopMeasurementHandler> {
        self.measurement.clone()
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

/// Cloneable event channel handed to each Desktop-native element instance.
///
/// Events are queued and delivered at the next runtime frame boundary. This
/// keeps native callbacks from re-entering Rust application code.
#[derive(Clone)]
pub struct DesktopEventEmitter(Arc<dyn Fn(DesktopNativeEvent) + Send + Sync>);

impl DesktopEventEmitter {
    pub(crate) fn new(emit: impl Fn(DesktopNativeEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(emit))
    }

    /// Queues a declared element event for Rust delivery.
    pub fn emit(&self, event: DesktopNativeEvent) {
        (self.0)(event);
    }
}

impl Default for DesktopEventEmitter {
    fn default() -> Self {
        Self(Arc::new(|_| {}))
    }
}

impl fmt::Debug for DesktopEventEmitter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DesktopEventEmitter(..)")
    }
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

/// Editing keys normalized by the shared Desktop window shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopTextInputKey {
    /// Delete the previous grapheme or selection.
    Backspace,
    /// Delete the next grapheme or selection.
    Delete,
    /// Move the caret one logical character left.
    ArrowLeft,
    /// Move the caret one logical character right.
    ArrowRight,
    /// Move the caret to the start of the value.
    Home,
    /// Move the caret to the end of the value.
    End,
    /// Insert a newline or submit a single-line field.
    Enter,
}

/// OS text-service input delivered only to the focused Desktop native element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopTextInputEvent {
    /// Replace the current selection or marked range with committed text.
    Commit(String),
    /// Update the current IME marked text. An empty string cancels composition.
    Preedit {
        /// Current marked text.
        text: String,
        /// Cursor range inside `text`, expressed as UTF-8 byte offsets.
        cursor: Option<(usize, usize)>,
    },
    /// A non-text editing key.
    Key {
        /// Semantic key.
        key: DesktopTextInputKey,
        /// Whether movement extends the current selection.
        shift: bool,
    },
    /// Select the complete value.
    SelectAll,
    /// Delete the selection after the Host copied it to the clipboard.
    Cut,
    /// Insert clipboard text at the current selection.
    Paste(String),
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

    /// Executes one negotiated one-way command.
    fn invoke_command(&mut self, command: CommandId, arguments: &WhiskerValue);

    /// Applies resolved inherited text style when the element schema declares
    /// text-style consumption.
    fn set_text_style(&mut self, _style: &WhiskerTextStyle) {}

    /// Produces element-owned pixels for the requested physical bounds.
    ///
    /// Implementations should cache by `(generation, width, height)` and may
    /// return `None` for empty or temporarily unavailable content. The default
    /// keeps state-only custom elements allocation-free.
    fn rasterize(&self, _width: u32, _height: u32) -> Option<DesktopRaster> {
        None
    }

    /// Produces element-owned pixels with the current logical-to-physical
    /// scale. Existing image-like elements can keep implementing
    /// [`Self::rasterize`]; text controls use this hook for crisp glyphs.
    fn rasterize_scaled(&self, width: u32, height: u32, _scale: f32) -> Option<DesktopRaster> {
        self.rasterize(width, height)
    }

    /// Whether this element contributes raster content at all.
    fn has_raster_content(&self) -> bool {
        false
    }

    /// Whether this native element participates in Desktop text editing.
    fn accepts_text_input(&self) -> bool {
        false
    }

    /// Whether this element currently owns keyboard and IME focus.
    fn text_input_focused(&self) -> bool {
        false
    }

    /// Changes keyboard and IME focus for this element.
    fn set_text_input_focus(&mut self, _focused: bool) {}

    /// Applies one normalized keyboard, clipboard, or IME edit.
    fn handle_text_input(&mut self, _event: &DesktopTextInputEvent) {}

    /// Returns selected text for the Host clipboard bridge.
    fn selected_text(&self) -> Option<String> {
        None
    }

    /// Whether this element owns transient vertical scroll state.
    fn is_scroll_container(&self) -> bool {
        false
    }

    /// Whether wheel deltas should advance the horizontal axis.
    fn scroll_horizontal(&self) -> bool {
        false
    }

    /// Direct-child snap anchor `(factor, logical offset)` when enabled.
    fn item_snap(&self) -> Option<(f64, f64)> {
        None
    }

    /// Whether a single scroll sequence must stop at the next snap point.
    fn snap_stop_always(&self) -> bool {
        false
    }

    /// Whether pointer/wheel gestures may change scroll state.
    fn scroll_enabled(&self) -> bool {
        true
    }
}

#[derive(Debug)]
struct DesktopScrollViewState {
    horizontal: bool,
    item_snap: Option<(f64, f64)>,
    snap_stop_always: bool,
    scroll_enabled: bool,
}

impl Default for DesktopScrollViewState {
    fn default() -> Self {
        Self {
            horizontal: false,
            item_snap: None,
            snap_stop_always: false,
            scroll_enabled: true,
        }
    }
}

/// Built-in element implementations contributed by the Desktop platform.
pub struct BuiltInElementModule;

#[WhiskerModule]
impl WhiskerModule for BuiltInElementModule {
    type Definition = DesktopModuleDefinition;

    fn definition() -> Self::Definition {
        DesktopModuleDefinition::new()
            .name("whisker.ui")
            .view(DesktopViewDefinition::new("whisker.ui/View", |_| ()))
            .view(DesktopViewDefinition::new("whisker.ui/Text", |_| ()).plain_text())
            .view(
                DesktopViewDefinition::new("whisker.ui/ScrollView", |_| {
                    DesktopScrollViewState::default()
                })
                .prop(
                    "scroll-orientation",
                    |state, value| {
                        state.horizontal =
                            matches!(value, WhiskerValue::String(value) if value == "horizontal");
                    },
                    |state| state.horizontal = false,
                )
                .prop(
                    "item-snap",
                    |state, value| {
                        let WhiskerValue::Map(value) = value else {
                            state.item_snap = None;
                            return;
                        };
                        let number = |name| match value.get(name) {
                            Some(WhiskerValue::Float(value)) => Some(*value),
                            Some(WhiskerValue::Int(value)) => Some(*value as f64),
                            _ => None,
                        };
                        state.item_snap = Some((
                            number("factor").unwrap_or(0.0).clamp(0.0, 1.0),
                            number("offset").unwrap_or(0.0),
                        ));
                    },
                    |state| state.item_snap = None,
                )
                .prop(
                    "scroll-snap-stop",
                    |state, value| {
                        state.snap_stop_always =
                            matches!(value, WhiskerValue::String(value) if value == "always");
                    },
                    |state| state.snap_stop_always = false,
                )
                .prop(
                    "enable-scroll",
                    |state, value| {
                        state.scroll_enabled = matches!(value, WhiskerValue::Bool(true));
                    },
                    |state| state.scroll_enabled = true,
                )
                .scroll_behavior(
                    |state| state.horizontal,
                    |state| state.item_snap,
                    |state| state.snap_stop_always,
                    |state| state.scroll_enabled,
                )
                // Scene owns the actual scroll offset; these declarations
                // negotiate the shared command IDs while Scene applies them.
                .command("scrollTo", |_, _| {})
                .command("scrollBy", |_, _| {})
                .event("scroll"),
            )
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
    pub(crate) fn reset_for_presentation_reuse(&mut self) {
        match self {
            Self::Text(text) => *text = None,
            Self::Native { text, .. } => *text = None,
            Self::Empty | Self::ScrollContainer => {}
        }
    }

    pub(crate) fn is_scroll_container(&self) -> bool {
        match self {
            Self::ScrollContainer => true,
            Self::Native { implementation, .. } => implementation.is_scroll_container(),
            Self::Empty | Self::Text(_) => false,
        }
    }

    pub(crate) fn scroll_horizontal(&self) -> bool {
        match self {
            Self::Native { implementation, .. } => implementation.scroll_horizontal(),
            Self::Empty | Self::Text(_) | Self::ScrollContainer => false,
        }
    }

    pub(crate) fn item_snap(&self) -> Option<(f64, f64)> {
        match self {
            Self::Native { implementation, .. } => implementation.item_snap(),
            Self::Empty | Self::Text(_) | Self::ScrollContainer => None,
        }
    }

    pub(crate) fn snap_stop_always(&self) -> bool {
        match self {
            Self::Native { implementation, .. } => implementation.snap_stop_always(),
            Self::Empty | Self::Text(_) | Self::ScrollContainer => false,
        }
    }

    pub(crate) fn scroll_enabled(&self) -> bool {
        match self {
            Self::Native { implementation, .. } => implementation.scroll_enabled(),
            Self::ScrollContainer => true,
            Self::Empty | Self::Text(_) => false,
        }
    }

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

    pub(crate) fn accepts_text_input(&self) -> bool {
        matches!(self, Self::Native { implementation, .. } if implementation.accepts_text_input())
    }

    pub(crate) fn text_input_focused(&self) -> bool {
        matches!(self, Self::Native { implementation, .. } if implementation.text_input_focused())
    }

    pub(crate) fn set_text_input_focus(&mut self, focused: bool) {
        if let Self::Native { implementation, .. } = self {
            implementation.set_text_input_focus(focused);
        }
    }

    pub(crate) fn handle_text_input(&mut self, event: &DesktopTextInputEvent) {
        if let Self::Native { implementation, .. } = self {
            implementation.handle_text_input(event);
        }
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        match self {
            Self::Native { implementation, .. } => implementation.selected_text(),
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

    pub(crate) fn set_text_style(
        &mut self,
        node: NodeId,
        style: &WhiskerTextStyle,
    ) -> Result<(), DesktopElementError> {
        match self {
            Self::Native { implementation, .. } => {
                implementation.set_text_style(style);
                Ok(())
            }
            Self::Empty | Self::Text(_) | Self::ScrollContainer => {
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
    ) -> Result<(), DesktopElementError> {
        match self {
            Self::Native { implementation, .. } => {
                implementation.invoke_command(command, arguments);
                Ok(())
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
        events: DesktopEventEmitter,
    ) -> Result<DesktopElementContent, DesktopElementError> {
        Ok(self.binding(element_type)?.factory.create(events))
    }

    pub(crate) fn is_builtin_presentation(&self, element_type: ElementTypeId) -> bool {
        self.binding(element_type).is_ok_and(|binding| {
            matches!(
                binding.registration.name.as_str(),
                "whisker.ui/View" | "whisker.ui/Text"
            )
        })
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

    pub(crate) fn measure(
        &self,
        request: &MeasurementRequest,
    ) -> Result<Option<MeasurementResponse>, DesktopElementError> {
        Ok(self
            .binding(request.element_type)?
            .factory
            .measurer
            .as_ref()
            .map(
                |measure| match measure(&WhiskerMeasureRequest::from(request)) {
                    Some(size) => MeasurementResponse::Ready {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        metrics: MeasurementMetrics::from_size(size),
                    },
                    None => MeasurementResponse::Unsupported {
                        key: request.key,
                        environment_epoch: request.environment_epoch,
                        reason: UnsupportedMeasurementReason::Feature,
                    },
                },
            ))
    }

    pub(crate) fn receives_text_style(
        &self,
        element_type: ElementTypeId,
    ) -> Result<bool, DesktopElementError> {
        Ok(self.binding(element_type)?.registration.text_style)
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

    pub(crate) fn command_name(
        &self,
        element_type: ElementTypeId,
        command: CommandId,
    ) -> Option<String> {
        self.binding(element_type)
            .ok()?
            .registration
            .command(command)
            .map(|schema| schema.name.clone())
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
mod tests;

mod module_definition;

use module_definition::DesktopMeasurementHandler;
pub use module_definition::{
    DesktopModuleDefinition, DesktopViewDefinition, DesktopViewImplementation,
};
