//! Runtime ownership of one retained semantic surface.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use crate::ElementRegistry;
use crate::ElementRegistryError;
use crate::background_resources::{
    BackgroundProjection, BackgroundResourceError, BackgroundResourceManager,
};
use crate::runtime::element::ElementTag;
use crate::runtime::value::WhiskerValue;
use crate::runtime::view::{BindType, DynRenderer, Element};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    ElementRegistration, ElementSchema, ElementValueKind, HitTestBehavior, InputEvent,
    InputEventError, MeasurementReady, NodeId, ResourceCommand, ResourceEvent, ResourceId,
    ResourceMessageError, SurfaceId,
};
use whisker_engine::whisker_style::{
    InheritedStyle, ResolvedNodeStyle, SpecifiedStyle, StyleEnvironment, StyleResolutionError,
    resolve_style,
};
use whisker_engine::{
    DeferredMeasurementApply, FrameSink, LayoutError, LayoutOptions, LayoutProgress,
    MeasurementProvider, PlainTextInput, SurfaceEngine, SurfaceError, SurfacePresentError,
};

/// A mutation emitted by `render!` that could not enter the retained surface.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeBindingError {
    /// A module attempted to publish an invalid or conflicting schema.
    ElementRegistry(ElementRegistryError),
    /// The runtime named an element handle unknown to this surface.
    UnknownElement {
        /// Missing runtime handle.
        element: Element,
    },
    /// A custom element has no registered provider on this surface.
    UnsupportedCustomElement {
        /// Requested element name.
        name: String,
    },
    /// A built-in authoring tag has no negotiated element registration.
    MissingElementRegistration {
        /// Tag that could not be mapped to a compact element type.
        tag: ElementTag,
    },
    /// A leaf element was given an ordinary scene child.
    ChildrenNotAllowed {
        /// Parent element whose schema declares no child mount target.
        parent: Element,
        /// Child rejected before entering the retained scene.
        child: Element,
    },
    /// A compatibility-only CSS string reached the typed renderer.
    UnsupportedRawStyle {
        /// Styled runtime handle.
        element: Element,
        /// Original CSS retained for diagnostics.
        css: String,
    },
    /// An attribute has not yet been mapped to a typed element property.
    UnsupportedAttribute {
        /// Target runtime handle.
        element: Element,
        /// Attribute name.
        name: String,
    },
    /// An element property or command received a value of the wrong shape.
    InvalidElementValue {
        /// Target runtime handle.
        element: Element,
        /// Property or command name.
        name: String,
        /// Top-level shape declared by the element contract.
        expected: ElementValueKind,
    },
    /// An element command was not declared by the negotiated contract.
    UnsupportedElementCommand {
        /// Target runtime handle.
        element: Element,
        /// Command name.
        name: String,
    },
    /// A raw-text helper was attached outside a Text element.
    InvalidRawTextParent {
        /// Raw-text runtime handle.
        element: Element,
        /// Requested parent.
        parent: Element,
    },
    /// The runtime selected a virtual raw-text node as the surface root.
    InvalidRoot {
        /// Invalid runtime handle.
        element: Element,
    },
    /// Runtime element handles exhausted their reserved `u32` range.
    ElementIdExhausted,
    /// Automatic paint resource IDs exhausted their non-zero `u64` range.
    ResourceIdExhausted,
    /// A typed background resource source was empty or otherwise malformed.
    InvalidBackgroundResourceSource,
    /// Typed style resolution failed.
    Style(StyleResolutionError),
    /// The retained scene or layout engine rejected the mutation.
    Surface(SurfaceError),
}

impl fmt::Display for RuntimeBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render binding error: {self:?}")
    }
}

impl Error for RuntimeBindingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ElementRegistry(error) => Some(error),
            Self::Style(error) => Some(error),
            Self::Surface(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StyleResolutionError> for RuntimeBindingError {
    fn from(error: StyleResolutionError) -> Self {
        Self::Style(error)
    }
}

impl From<SurfaceError> for RuntimeBindingError {
    fn from(error: SurfaceError) -> Self {
        Self::Surface(error)
    }
}

impl From<ElementRegistryError> for RuntimeBindingError {
    fn from(error: ElementRegistryError) -> Self {
        Self::ElementRegistry(error)
    }
}

impl From<BackgroundResourceError> for RuntimeBindingError {
    fn from(error: BackgroundResourceError) -> Self {
        match error {
            BackgroundResourceError::InvalidSource => Self::InvalidBackgroundResourceSource,
            BackgroundResourceError::ResourceIdExhausted => Self::ResourceIdExhausted,
        }
    }
}

/// Input rejected before or during Rust event routing.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeInputError {
    /// Input targeted a different surface.
    SurfaceMismatch {
        /// Surface owned by this runtime.
        expected: SurfaceId,
        /// Surface named by the Host event.
        received: SurfaceId,
    },
    /// Host timing or pointer geometry was invalid.
    InvalidInput(InputEventError),
    /// The Host named a node that is no longer live.
    UnknownTarget {
        /// Stale or invalid target.
        node: NodeId,
    },
    /// An explicit Host target was live but not mounted under this surface root.
    TargetOutsideRoot {
        /// Unmounted target supplied by the Host.
        target: NodeId,
        /// Current mounted root.
        root: NodeId,
    },
    /// Re-entrant Host delivery exceeded the bounded event queue.
    InputQueueFull {
        /// Maximum number of retained events.
        limit: usize,
    },
    /// No root has been mounted.
    MissingRoot,
    /// Retained scene hit testing failed.
    Binding(RuntimeBindingError),
}

impl fmt::Display for RuntimeInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime input error: {self:?}")
    }
}

impl Error for RuntimeInputError {}

/// Summary of one routed Host event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDispatch {
    /// Hit-tested or explicitly addressed target.
    pub target: Option<NodeId>,
    /// Whether at least one listener received the event.
    pub consumed: bool,
    /// Number of callbacks fired across capture and bubble phases.
    pub listener_count: usize,
    /// Whether a re-entrant Host callback was queued for the event boundary.
    pub queued: bool,
}

/// Resource-channel command or completion rejected before Host/runtime state
/// could change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeResourceError {
    /// The protocol message itself is malformed.
    InvalidMessage(ResourceMessageError),
    /// A load generation must increase monotonically for one resource ID.
    NonMonotonicGeneration {
        /// Resource whose current generation won the race.
        resource: ResourceId,
        /// Latest generation already accepted.
        current: u64,
        /// Older or duplicate generation that was rejected.
        received: u64,
    },
    /// A release or completion named a generation that was never loaded.
    UnknownGeneration {
        /// Named resource.
        resource: ResourceId,
        /// Named generation.
        generation: u64,
    },
    /// Public/manual resource traffic attempted to claim a runtime-owned ID.
    AutomaticResourceId {
        /// ID reserved permanently by automatic style lowering.
        resource: ResourceId,
    },
}

impl fmt::Display for RuntimeResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime resource error: {self:?}")
    }
}

impl Error for RuntimeResourceError {}

/// Whether one valid Host resource completion changed the current generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceEventApply {
    /// The completion belongs to the current unreleased generation.
    Applied,
    /// A replacement or release made the completion stale before it arrived.
    Stale,
}

/// Failure while driving layout for a surface populated through `render!`.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeLayoutError<MeasurementError> {
    /// A prior runtime mutation was rejected.
    Binding(RuntimeBindingError),
    /// The runtime has not called `set_root` yet.
    MissingRoot,
    /// Host measurement or retained layout failed.
    Measurement(LayoutError<MeasurementError>),
}

/// Failure while presenting a surface populated through `render!`.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimePresentError<SinkError> {
    /// A prior runtime mutation was rejected.
    Binding(RuntimeBindingError),
    /// Frame preparation, Host presentation, or acknowledgement failed.
    Present(SurfacePresentError<SinkError>),
}

impl<SinkError: fmt::Debug> fmt::Display for RuntimePresentError<SinkError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render presentation error: {self:?}")
    }
}

impl<SinkError: Error + 'static> Error for RuntimePresentError<SinkError> {}

/// Output of one complete runtime frame.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeFrame {
    /// Measurement and layout progress for this frame.
    pub layout: LayoutProgress,
    /// Host acknowledgement, absent when blocking measurement withheld paint.
    pub presentation: Option<whisker_engine::whisker_protocol::ApplyResult>,
}

/// Failure while measuring, laying out, or presenting one runtime frame.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeFrameError<MeasurementError, SinkError> {
    /// Measurement or layout failed.
    Layout(RuntimeLayoutError<MeasurementError>),
    /// Frame presentation failed.
    Present(RuntimePresentError<SinkError>),
}

impl<MeasurementError: fmt::Debug, SinkError: fmt::Debug> fmt::Display
    for RuntimeFrameError<MeasurementError, SinkError>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime frame error: {self:?}")
    }
}

impl<MeasurementError, SinkError> Error for RuntimeFrameError<MeasurementError, SinkError>
where
    MeasurementError: Error + 'static,
    SinkError: Error + 'static,
{
}

impl<MeasurementError: fmt::Debug> fmt::Display for RuntimeLayoutError<MeasurementError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker render layout error: {self:?}")
    }
}

impl<MeasurementError: Error + 'static> Error for RuntimeLayoutError<MeasurementError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Binding(error) => Some(error),
            Self::Measurement(error) => Some(error),
            Self::MissingRoot => None,
        }
    }
}

/// First-class runtime for one retained surface populated by [`render!`](crate::render).
///
/// Clones share one single-threaded surface. Install [`Self::renderer`] through
/// `whisker_runtime::view::with_installed_renderer`, build the declarative tree,
/// call `set_root`, and then drive Host measurement through this handle.
#[derive(Clone)]
pub struct SurfaceRuntime {
    state: Rc<RefCell<BindingState>>,
}

/// Returns the built-in element contracts used by a default surface.
///
/// Hosts normally obtain the same values from
/// [`SurfaceRuntime::element_registrations`]. This helper is useful for
/// Host-only conformance tests that intentionally do not mount a runtime.
pub fn standard_element_registrations() -> Vec<ElementRegistration> {
    ElementRegistry::standard().registrations().to_vec()
}

impl SurfaceRuntime {
    /// Creates an empty renderer-backed surface for one style environment.
    pub fn new(surface: SurfaceId, environment: StyleEnvironment) -> Self {
        Self::with_element_registry(surface, environment, ElementRegistry::standard())
    }

    /// Creates a surface with a registry normalized during application bootstrap.
    ///
    /// Built-in and module-provided elements in this registry share the same
    /// compact-ID allocation and retained scene path.
    pub fn with_element_registry(
        surface: SurfaceId,
        environment: StyleEnvironment,
        registry: ElementRegistry,
    ) -> Self {
        Self {
            state: Rc::new(RefCell::new(BindingState {
                surface: SurfaceEngine::new(surface),
                environment,
                registry,
                next_element: 0,
                elements: HashMap::new(),
                node_elements: HashMap::new(),
                root: None,
                error: None,
                resource_commands: VecDeque::new(),
                resource_generations: HashMap::new(),
                known_resource_generations: HashSet::new(),
                released_resource_generations: HashSet::new(),
                resource_events: HashMap::new(),
                background_resources: BackgroundResourceManager::default(),
            })),
        }
    }

    /// Returns a renderer sharing this surface, ready for runtime installation.
    pub fn renderer(&self) -> Box<dyn DynRenderer> {
        Box::new(self.clone())
    }

    /// Returns the semantic surface identifier.
    pub fn surface(&self) -> SurfaceId {
        self.state.borrow().surface.surface()
    }

    /// Returns the element contracts negotiated by this surface runtime.
    ///
    /// A Host binds this snapshot before accepting the first frame. Built-in
    /// and future module-provided elements use the same registration values;
    /// `ElementTag` discriminants are not wire element IDs.
    pub fn element_registrations(&self) -> Vec<ElementRegistration> {
        self.state.borrow().registry.registrations().to_vec()
    }

    /// Returns the root selected by the runtime, when available.
    pub fn root(&self) -> Option<NodeId> {
        self.state.borrow().root
    }

    /// Returns the first rejected runtime mutation without clearing it.
    pub fn binding_error(&self) -> Option<RuntimeBindingError> {
        self.state.borrow().error.clone()
    }

    /// Returns the environment used for viewport-relative style resolution.
    pub fn environment(&self) -> StyleEnvironment {
        self.state.borrow().environment
    }

    /// Re-resolves every retained style against current Host viewport metrics.
    ///
    /// The update is prepared against a cloned surface and committed only after
    /// every node resolves successfully. This keeps a rejected Host environment
    /// from partially changing layout, paint, or text measurement state.
    pub fn update_environment(
        &self,
        environment: StyleEnvironment,
    ) -> Result<bool, RuntimeBindingError> {
        let mut state = self.state.borrow_mut();
        state.update_environment(environment)
    }

    /// Hit-tests and routes one Host-normalized event through Rust listeners.
    pub fn dispatch_input(&self, event: &InputEvent) -> Result<InputDispatch, RuntimeInputError> {
        let (target, firings, body) = {
            let state = self.state.borrow();
            state.ensure_valid().map_err(RuntimeInputError::Binding)?;
            if event.surface != state.surface.surface() {
                return Err(RuntimeInputError::SurfaceMismatch {
                    expected: state.surface.surface(),
                    received: event.surface,
                });
            }
            event.validate().map_err(RuntimeInputError::InvalidInput)?;
            let root = state.root.ok_or(RuntimeInputError::MissingRoot)?;
            let target = if let Some(target) = event.target {
                if state.surface.node(target).is_none() {
                    return Err(RuntimeInputError::UnknownTarget { node: target });
                }
                Some(target)
            } else if let Some(pointer) = event.pointer {
                if let Some(captured) = state.surface.pointer_capture_target(pointer.id) {
                    Some(captured)
                } else {
                    state
                        .surface
                        .hit_test(root, pointer.position)
                        .map_err(RuntimeBindingError::from)
                        .map_err(RuntimeInputError::Binding)?
                }
            } else {
                None
            };
            let Some(target) = target else {
                return Ok(InputDispatch::default());
            };
            let event_name = event.kind.name(event.pointer.map(|pointer| pointer.kind));
            let firings = state.plan_event(root, target, event_name)?;
            (target, firings, input_body(event, target))
        };

        let listener_count = firings.len();
        for (current_target, callback) in firings {
            callback(with_current_target(&body, current_target));
        }
        Ok(InputDispatch {
            target: Some(target),
            consumed: listener_count > 0,
            listener_count,
            queued: false,
        })
    }

    /// Applies one deferred Host measurement and invalidates its layout users.
    pub fn apply_measurement_ready(
        &self,
        ready: &MeasurementReady,
    ) -> Result<DeferredMeasurementApply, RuntimeBindingError> {
        let mut state = self.state.borrow_mut();
        state.ensure_valid()?;
        state
            .surface
            .apply_measurement_ready(ready)
            .map_err(RuntimeBindingError::from)
    }

    /// Enqueues one typed non-frame resource command for the Host. Loads for
    /// one ID must use a strictly increasing generation.
    pub fn enqueue_resource_command(
        &self,
        command: ResourceCommand,
    ) -> Result<(), RuntimeResourceError> {
        let mut state = self.state.borrow_mut();
        state.enqueue_resource_command(command, false)?;
        crate::runtime::runtime_wake::wake_runtime();
        Ok(())
    }

    /// Drains pending resource commands in enqueue order. Hosts call this at a
    /// short runtime boundary and copy borrowed byte payloads only once.
    pub fn take_resource_commands(&self) -> Vec<ResourceCommand> {
        self.state
            .borrow_mut()
            .resource_commands
            .drain(..)
            .collect()
    }

    /// Applies one typed Host completion with generation safety.
    pub fn apply_resource_event(
        &self,
        event: &ResourceEvent,
    ) -> Result<ResourceEventApply, RuntimeResourceError> {
        event
            .validate()
            .map_err(RuntimeResourceError::InvalidMessage)?;
        let (resource, generation) = match event {
            ResourceEvent::Ready {
                resource,
                generation,
                ..
            }
            | ResourceEvent::Failed {
                resource,
                generation,
                ..
            } => (*resource, *generation),
        };
        let mut state = self.state.borrow_mut();
        if !state
            .known_resource_generations
            .contains(&(resource, generation))
        {
            return Err(RuntimeResourceError::UnknownGeneration {
                resource,
                generation,
            });
        }
        if state.resource_generations.get(&resource).copied() != Some(generation)
            || state
                .released_resource_generations
                .contains(&(resource, generation))
        {
            return Ok(ResourceEventApply::Stale);
        }
        if state.background_resources.owns(resource) {
            // This only changes manager state. Scene mutation is intentionally
            // deferred to drive_layout/present, outside the Host callback.
            debug_assert!(state.background_resources.apply_event(event));
        }
        state
            .resource_events
            .insert((resource, generation), event.clone());
        Ok(ResourceEventApply::Applied)
    }

    /// Returns the accepted completion for one current resource generation.
    pub fn resource_event(&self, resource: ResourceId, generation: u64) -> Option<ResourceEvent> {
        self.state
            .borrow()
            .resource_events
            .get(&(resource, generation))
            .cloned()
    }

    /// Runs Taffy and all synchronously available Host measurements.
    pub fn drive_layout<Provider: MeasurementProvider>(
        &self,
        viewport: LayoutSize,
        environment_epoch: u64,
        provider: &mut Provider,
        options: LayoutOptions,
    ) -> Result<LayoutProgress, RuntimeLayoutError<Provider::Error>> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = state.error.clone() {
            return Err(RuntimeLayoutError::Binding(error));
        }
        state
            .flush_background_projections()
            .map_err(RuntimeLayoutError::Binding)?;
        let root = state.root.ok_or(RuntimeLayoutError::MissingRoot)?;
        state
            .surface
            .drive_layout(root, viewport, environment_epoch, provider, options)
            .map_err(RuntimeLayoutError::Measurement)
    }

    /// Presents the next transaction and records the Host acknowledgement.
    pub fn present<Sink: FrameSink>(
        &self,
        viewport_epoch: u32,
        sink: &mut Sink,
    ) -> Result<
        Option<whisker_engine::whisker_protocol::ApplyResult>,
        RuntimePresentError<Sink::Error>,
    > {
        let mut state = self.state.borrow_mut();
        state.ensure_valid().map_err(RuntimePresentError::Binding)?;
        state
            .flush_background_projections()
            .map_err(RuntimePresentError::Binding)?;
        let presentation = state
            .surface
            .present(viewport_epoch, sink)
            .map_err(RuntimePresentError::Present)?;
        if matches!(
            presentation,
            Some(whisker_engine::whisker_protocol::ApplyResult::Accepted { .. })
        ) {
            let commands = state.background_resources.accept_frame();
            state.enqueue_automatic_commands(commands);
        }
        Ok(presentation)
    }

    /// Runs Host measurement, final layout, and transactional presentation.
    pub fn render_frame<Provider: MeasurementProvider, Sink: FrameSink>(
        &self,
        viewport: LayoutSize,
        environment_epoch: u64,
        viewport_epoch: u32,
        provider: &mut Provider,
        sink: &mut Sink,
        options: LayoutOptions,
    ) -> Result<RuntimeFrame, RuntimeFrameError<Provider::Error, Sink::Error>> {
        let layout = self
            .drive_layout(viewport, environment_epoch, provider, options)
            .map_err(RuntimeFrameError::Layout)?;
        let presentation = if layout.has_layout() {
            self.present(viewport_epoch, sink)
                .map_err(RuntimeFrameError::Present)?
        } else {
            None
        };
        Ok(RuntimeFrame {
            layout,
            presentation,
        })
    }
}

#[derive(Clone)]
struct BoundElement {
    kind: BoundElementKind,
    node: Option<NodeId>,
    parent: Option<Element>,
    children: Vec<Element>,
    base_specified: SpecifiedStyle,
    specified: SpecifiedStyle,
    resolved: Option<ResolvedNodeStyle>,
    text: Option<PlainTextInput>,
    raw_text: String,
    listeners: HashMap<String, Vec<RuntimeListener>>,
}

#[derive(Clone)]
enum BoundElementKind {
    RawText,
    Registered { registration: ElementRegistration },
}

impl BoundElementKind {
    fn is_raw_text(&self) -> bool {
        matches!(self, Self::RawText)
    }

    fn accepts_plain_text(&self) -> bool {
        matches!(
            self,
            Self::Registered { registration }
                if registration.child_policy.accepts_plain_text()
        )
    }

    fn accepts_elements(&self) -> bool {
        match self {
            Self::RawText => false,
            Self::Registered { registration } => registration.child_policy.accepts_elements(),
        }
    }

    fn registration(&self) -> Option<&ElementRegistration> {
        match self {
            Self::RawText => None,
            Self::Registered { registration } => Some(registration),
        }
    }
}

impl BoundElement {
    fn effective_specified(&self) -> SpecifiedStyle {
        self.base_specified.clone().merge(self.specified.clone())
    }
}

#[derive(Clone)]
struct RuntimeListener {
    bind_type: BindType,
    callback: Rc<dyn Fn(WhiskerValue) + 'static>,
}

type PlannedListener = (NodeId, Rc<dyn Fn(WhiskerValue) + 'static>);

struct BindingState {
    surface: SurfaceEngine,
    environment: StyleEnvironment,
    registry: ElementRegistry,
    next_element: u32,
    elements: HashMap<Element, BoundElement>,
    node_elements: HashMap<NodeId, Element>,
    root: Option<NodeId>,
    error: Option<RuntimeBindingError>,
    resource_commands: VecDeque<ResourceCommand>,
    resource_generations: HashMap<ResourceId, u64>,
    known_resource_generations: HashSet<(ResourceId, u64)>,
    released_resource_generations: HashSet<(ResourceId, u64)>,
    resource_events: HashMap<(ResourceId, u64), ResourceEvent>,
    background_resources: BackgroundResourceManager,
}

struct EnvironmentStyleUpdate {
    element: Element,
    node: NodeId,
    resolved: ResolvedNodeStyle,
    text: Option<PlainTextInput>,
}

impl BindingState {
    fn ensure_valid(&self) -> Result<(), RuntimeBindingError> {
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn record(&mut self, result: Result<(), RuntimeBindingError>) {
        match result {
            Ok(()) => crate::runtime::runtime_wake::wake_runtime(),
            Err(error) if self.error.is_none() => self.error = Some(error),
            Err(_) => {}
        }
    }

    fn enqueue_resource_command(
        &mut self,
        command: ResourceCommand,
        automatic: bool,
    ) -> Result<(), RuntimeResourceError> {
        command
            .validate()
            .map_err(RuntimeResourceError::InvalidMessage)?;
        let resource = match &command {
            ResourceCommand::Load(request) => request.resource,
            ResourceCommand::Release { resource, .. } => *resource,
        };
        if !automatic && self.background_resources.owns(resource) {
            return Err(RuntimeResourceError::AutomaticResourceId { resource });
        }
        match &command {
            ResourceCommand::Load(request) => {
                if let Some(current) = self.resource_generations.get(&request.resource).copied()
                    && request.generation <= current
                {
                    return Err(RuntimeResourceError::NonMonotonicGeneration {
                        resource: request.resource,
                        current,
                        received: request.generation,
                    });
                }
                self.resource_generations
                    .insert(request.resource, request.generation);
                self.known_resource_generations
                    .insert((request.resource, request.generation));
            }
            ResourceCommand::Release {
                resource,
                generation,
            } => {
                if !self
                    .known_resource_generations
                    .contains(&(*resource, *generation))
                {
                    return Err(RuntimeResourceError::UnknownGeneration {
                        resource: *resource,
                        generation: *generation,
                    });
                }
                self.released_resource_generations
                    .insert((*resource, *generation));
            }
        }
        self.resource_commands.push_back(command);
        Ok(())
    }

    fn enqueue_automatic_commands(&mut self, commands: Vec<ResourceCommand>) {
        for command in commands {
            self.enqueue_resource_command(command, true)
                .expect("automatic resource commands preserve lifecycle invariants");
        }
    }

    fn externally_used_resource_ids(&self) -> HashSet<ResourceId> {
        self.resource_generations.keys().copied().collect()
    }

    fn flush_background_projections(&mut self) -> Result<(), RuntimeBindingError> {
        let projections = self.background_resources.dirty_projections();
        if projections.is_empty() {
            return Ok(());
        }
        let mut surface = self.surface.clone();
        for BackgroundProjection { node, layers } in &projections {
            // A node can be released between completion delivery and the next
            // safe boundary. `remove_nodes` normally removes its dirty bit,
            // while this guard keeps the boundary robust to duplicate release.
            if surface.node(*node).is_some() {
                surface.set_background_layers(*node, layers.clone())?;
            }
        }
        self.surface = surface;
        self.background_resources
            .commit_dirty_projections(&projections);
        Ok(())
    }

    fn update_environment(
        &mut self,
        environment: StyleEnvironment,
    ) -> Result<bool, RuntimeBindingError> {
        self.ensure_valid()?;
        // Validate even an empty surface before accepting Host-owned metrics.
        resolve_style(&SpecifiedStyle::new(), None, environment)?;
        if environment == self.environment {
            return Ok(false);
        }

        let roots = self
            .elements
            .iter()
            .filter_map(|(element, entry)| {
                (entry.node.is_some() && entry.parent.is_none()).then_some(*element)
            })
            .collect::<Vec<_>>();
        let mut updates = Vec::with_capacity(self.node_elements.len());
        for root in roots {
            self.resolve_environment_subtree(root, None, environment, &mut updates)?;
        }

        let mut surface = self.surface.clone();
        let mut background_resources = self.background_resources.clone();
        let externally_used = self.externally_used_resource_ids();
        let mut resource_commands = Vec::new();
        for update in &updates {
            surface.update_computed_style(update.node, update.resolved.computed())?;
            let background = background_resources.reconcile_node(
                update.node,
                &update.resolved.computed().paint().background_images,
                &update.resolved.computed().paint().background_layers,
                &externally_used,
            )?;
            surface.set_background_layers(update.node, background.layers)?;
            resource_commands.extend(background.commands);
            if let Some(text) = &update.text {
                surface.set_plain_text(update.node, text, update.resolved.computed())?;
            }
        }

        self.surface = surface;
        self.background_resources = background_resources;
        self.enqueue_automatic_commands(resource_commands);
        self.environment = environment;
        for update in updates {
            self.element_mut(update.element)?.resolved = Some(update.resolved);
        }
        Ok(true)
    }

    fn resolve_environment_subtree(
        &self,
        element: Element,
        parent: Option<&InheritedStyle>,
        environment: StyleEnvironment,
        updates: &mut Vec<EnvironmentStyleUpdate>,
    ) -> Result<(), RuntimeBindingError> {
        let entry = self.element(element)?;
        let Some(node) = entry.node else {
            return Ok(());
        };
        let resolved = resolve_style(&entry.effective_specified(), parent, environment)?;
        let children = entry.children.clone();
        updates.push(EnvironmentStyleUpdate {
            element,
            node,
            resolved: resolved.clone(),
            text: entry.text.clone(),
        });
        for child in children {
            if self.element(child)?.node.is_some() {
                self.resolve_environment_subtree(
                    child,
                    Some(resolved.inherited_for_children()),
                    environment,
                    updates,
                )?;
            }
        }
        Ok(())
    }

    fn allocate(&mut self, tag: ElementTag) -> Result<Element, RuntimeBindingError> {
        if tag == ElementTag::RawText {
            return self.allocate_registration(None);
        }
        let registration = self
            .registry
            .registration_for_builtin(tag)
            .cloned()
            .ok_or(RuntimeBindingError::MissingElementRegistration { tag })?;
        self.allocate_registration(Some(registration))
    }

    fn allocate_named(&mut self, name: &str) -> Result<Element, RuntimeBindingError> {
        let registration = self
            .registry
            .registration_for_name(name)
            .cloned()
            .ok_or_else(|| RuntimeBindingError::UnsupportedCustomElement {
                name: name.to_owned(),
            })?;
        self.allocate_registration(Some(registration))
    }

    fn allocate_registration(
        &mut self,
        registration: Option<ElementRegistration>,
    ) -> Result<Element, RuntimeBindingError> {
        if self.next_element == u32::MAX {
            return Err(RuntimeBindingError::ElementIdExhausted);
        }
        let handle = Element::from_raw(self.next_element);
        self.next_element += 1;
        let (kind, node, base_specified, resolved, text) = if let Some(registration) = registration
        {
            let base_specified = self.registry.base_style(&registration).clone();
            let resolved = resolve_style(&base_specified, None, self.environment)?;
            let node = self.surface.create_node(
                registration.element_type,
                resolved.computed().layout().clone(),
            )?;
            let text = registration
                .child_policy
                .accepts_plain_text()
                .then(|| PlainTextInput::new(""));
            (
                BoundElementKind::Registered { registration },
                Some(node),
                base_specified,
                Some(resolved),
                text,
            )
        } else {
            (
                BoundElementKind::RawText,
                None,
                SpecifiedStyle::new(),
                None,
                None,
            )
        };
        self.elements.insert(
            handle,
            BoundElement {
                kind,
                node,
                parent: None,
                children: Vec::new(),
                base_specified,
                specified: SpecifiedStyle::new(),
                resolved,
                text,
                raw_text: String::new(),
                listeners: HashMap::new(),
            },
        );
        if let Some(node) = node {
            self.node_elements.insert(node, handle);
        }
        Ok(handle)
    }

    fn plan_event(
        &self,
        root: NodeId,
        target: NodeId,
        event_name: &str,
    ) -> Result<Vec<PlannedListener>, RuntimeInputError> {
        let mut chain = Vec::new();
        let mut current = Some(target);
        while let Some(node) = current {
            chain.push(node);
            current = self
                .surface
                .node(node)
                .ok_or(RuntimeInputError::UnknownTarget { node })?
                .parent();
        }
        if chain.last() != Some(&root) {
            return Err(RuntimeInputError::TargetOutsideRoot { target, root });
        }

        let listeners_for = |node: NodeId| {
            self.node_elements
                .get(&node)
                .and_then(|element| self.elements.get(element))
                .and_then(|element| element.listeners.get(event_name))
                .map(Vec::as_slice)
                .unwrap_or(&[])
        };
        let mut firings = Vec::new();
        let mut capture_caught = false;
        for node in chain.iter().rev().copied() {
            let mut stop = false;
            for listener in listeners_for(node) {
                match listener.bind_type {
                    BindType::CaptureCatch => {
                        firings.push((node, Rc::clone(&listener.callback)));
                        capture_caught = true;
                        stop = true;
                    }
                    BindType::CaptureBind => {
                        firings.push((node, Rc::clone(&listener.callback)));
                    }
                    BindType::Bind | BindType::Catch => {}
                }
            }
            if stop {
                break;
            }
        }
        if !capture_caught {
            for node in chain {
                let mut stop = false;
                for listener in listeners_for(node) {
                    match listener.bind_type {
                        BindType::Catch => {
                            firings.push((node, Rc::clone(&listener.callback)));
                            stop = true;
                        }
                        BindType::Bind => {
                            firings.push((node, Rc::clone(&listener.callback)));
                        }
                        BindType::CaptureBind | BindType::CaptureCatch => {}
                    }
                }
                if stop {
                    break;
                }
            }
        }
        Ok(firings)
    }

    fn element(&self, element: Element) -> Result<&BoundElement, RuntimeBindingError> {
        self.elements
            .get(&element)
            .ok_or(RuntimeBindingError::UnknownElement { element })
    }

    fn element_mut(&mut self, element: Element) -> Result<&mut BoundElement, RuntimeBindingError> {
        self.elements
            .get_mut(&element)
            .ok_or(RuntimeBindingError::UnknownElement { element })
    }

    fn apply_subtree(&mut self, element: Element) -> Result<(), RuntimeBindingError> {
        let parent_style = self
            .element(element)?
            .parent
            .and_then(|parent| self.elements.get(&parent))
            .and_then(|parent| parent.resolved.as_ref())
            .map(|resolved| resolved.inherited_for_children().clone());
        let mut surface = self.surface.clone();
        let mut background_resources = self.background_resources.clone();
        let externally_used = self.externally_used_resource_ids();
        let mut updates = Vec::new();
        let mut resource_commands = Vec::new();
        self.prepare_subtree(
            element,
            parent_style.as_ref(),
            &mut surface,
            &mut background_resources,
            &externally_used,
            &mut updates,
            &mut resource_commands,
        )?;

        self.surface = surface;
        self.background_resources = background_resources;
        for (element, resolved) in updates {
            self.element_mut(element)?.resolved = Some(resolved);
        }
        self.enqueue_automatic_commands(resource_commands);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_subtree(
        &self,
        element: Element,
        parent_style: Option<&InheritedStyle>,
        surface: &mut SurfaceEngine,
        background_resources: &mut BackgroundResourceManager,
        externally_used: &HashSet<ResourceId>,
        updates: &mut Vec<(Element, ResolvedNodeStyle)>,
        resource_commands: &mut Vec<ResourceCommand>,
    ) -> Result<(), RuntimeBindingError> {
        let entry = self.element(element)?;
        let Some(node) = entry.node else {
            return Ok(());
        };
        let resolved = resolve_style(&entry.effective_specified(), parent_style, self.environment)?;
        surface.update_computed_style(node, resolved.computed())?;
        let background = background_resources.reconcile_node(
            node,
            &resolved.computed().paint().background_images,
            &resolved.computed().paint().background_layers,
            externally_used,
        )?;
        surface.set_background_layers(node, background.layers)?;
        resource_commands.extend(background.commands);
        if let Some(text) = &entry.text {
            surface.set_plain_text(node, text, resolved.computed())?;
        }
        let children = entry.children.clone();
        updates.push((element, resolved.clone()));
        for child in children {
            if self.element(child)?.node.is_some() {
                self.prepare_subtree(
                    child,
                    Some(resolved.inherited_for_children()),
                    surface,
                    background_resources,
                    externally_used,
                    updates,
                    resource_commands,
                )?;
            }
        }
        Ok(())
    }

    fn surface_subtree(&self, root: NodeId) -> Vec<NodeId> {
        let mut nodes = Vec::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            let Some(entry) = self.surface.node(node) else {
                continue;
            };
            nodes.push(node);
            pending.extend(entry.children().iter().copied());
        }
        nodes
    }

    fn refresh_text(&mut self, text_element: Element) -> Result<(), RuntimeBindingError> {
        if !self.element(text_element)?.kind.accepts_plain_text() {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: text_element,
                parent: text_element,
            });
        }
        let children = self.element(text_element)?.children.clone();
        let mut value = String::new();
        for child in children {
            let child = self.element(child)?;
            if child.kind.is_raw_text() {
                value.push_str(&child.raw_text);
            }
        }
        self.element_mut(text_element)?
            .text
            .as_mut()
            .expect("Text elements always retain plain-text input")
            .text = value;
        self.apply_subtree(text_element)
    }

    fn insert(
        &mut self,
        parent: Element,
        child: Element,
        before: Option<Element>,
    ) -> Result<(), RuntimeBindingError> {
        let parent_entry = self.element(parent)?;
        let child_entry = self.element(child)?;
        if parent_entry.node.is_none() || child_entry.parent.is_some() {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: child,
                parent,
            });
        }
        if child_entry.kind.is_raw_text() && !parent_entry.kind.accepts_plain_text() {
            return Err(RuntimeBindingError::InvalidRawTextParent {
                element: child,
                parent,
            });
        }
        if !child_entry.kind.is_raw_text() && !parent_entry.kind.accepts_elements() {
            return Err(RuntimeBindingError::ChildrenNotAllowed { parent, child });
        }
        let position = match before {
            Some(reference) => self
                .element(parent)?
                .children
                .iter()
                .position(|candidate| *candidate == reference)
                .ok_or(RuntimeBindingError::UnknownElement { element: reference })?,
            None => self.element(parent)?.children.len(),
        };
        let scene_index = self.element(parent)?.children[..position]
            .iter()
            .filter(|candidate| {
                self.elements
                    .get(candidate)
                    .is_some_and(|entry| entry.node.is_some())
            })
            .count() as u32;
        self.element_mut(parent)?.children.insert(position, child);
        self.element_mut(child)?.parent = Some(parent);
        if let Some(child_node) = self.element(child)?.node {
            let parent_node = self
                .element(parent)?
                .node
                .expect("validated scene parent has a node");
            self.surface
                .insert_child(parent_node, child_node, scene_index)?;
            self.apply_subtree(child)?;
        } else {
            self.refresh_text(parent)?;
        }
        Ok(())
    }

    fn detach(&mut self, parent: Element, child: Element) -> Result<(), RuntimeBindingError> {
        let position = self
            .element(parent)?
            .children
            .iter()
            .position(|candidate| *candidate == child)
            .ok_or(RuntimeBindingError::UnknownElement { element: child })?;
        self.element_mut(parent)?.children.remove(position);
        self.element_mut(child)?.parent = None;
        if let Some(child_node) = self.element(child)?.node {
            let parent_node = self
                .element(parent)?
                .node
                .expect("validated scene parent has a node");
            self.surface.remove_child(parent_node, child_node)?;
            self.apply_subtree(child)?;
        } else {
            self.refresh_text(parent)?;
        }
        Ok(())
    }

    fn set_attribute(
        &mut self,
        element: Element,
        name: &str,
        value: &str,
    ) -> Result<(), RuntimeBindingError> {
        let kind = self.element(element)?.kind.clone();
        if kind.is_raw_text() && name == "text" {
            self.element_mut(element)?.raw_text = value.to_owned();
            if let Some(parent) = self.element(element)?.parent {
                self.refresh_text(parent)?;
            }
            return Ok(());
        }
        if kind.accepts_plain_text() && name == "text-maxline" {
            let max_lines = value.parse::<i32>().ok().and_then(|value| {
                if value > 0 {
                    u32::try_from(value).ok()
                } else {
                    None
                }
            });
            self.element_mut(element)?
                .text
                .as_mut()
                .expect("Text elements always retain plain-text input")
                .max_lines = max_lines;
            self.apply_subtree(element)?;
            return Ok(());
        }
        self.set_property_value(element, name, WhiskerValue::String(value.to_owned()))
    }

    fn set_property_value(
        &mut self,
        element: Element,
        name: &str,
        value: WhiskerValue,
    ) -> Result<(), RuntimeBindingError> {
        let (node, property, expected) = {
            let entry = self.element(element)?;
            let registration = entry.kind.registration().ok_or_else(|| {
                RuntimeBindingError::UnsupportedAttribute {
                    element,
                    name: name.to_owned(),
                }
            })?;
            let property = registration.property_named(name).ok_or_else(|| {
                RuntimeBindingError::UnsupportedAttribute {
                    element,
                    name: name.to_owned(),
                }
            })?;
            (
                entry
                    .node
                    .expect("registered elements always own scene nodes"),
                property.property,
                property.value,
            )
        };
        if !expected.accepts(&value) {
            return Err(RuntimeBindingError::InvalidElementValue {
                element,
                name: name.to_owned(),
                expected,
            });
        }
        self.surface.set_property(node, property, value)?;
        Ok(())
    }

    fn invoke_command(
        &mut self,
        element: Element,
        name: &str,
        params: &WhiskerValue,
    ) -> Result<WhiskerValue, RuntimeBindingError> {
        let (node, command, expected) = {
            let entry = self.element(element)?;
            let registration = entry.kind.registration().ok_or_else(|| {
                RuntimeBindingError::UnsupportedElementCommand {
                    element,
                    name: name.to_owned(),
                }
            })?;
            let command = registration.command_named(name).ok_or_else(|| {
                RuntimeBindingError::UnsupportedElementCommand {
                    element,
                    name: name.to_owned(),
                }
            })?;
            (
                entry
                    .node
                    .expect("registered elements always own scene nodes"),
                command.command,
                command.arguments,
            )
        };
        let arguments = command_arguments(params, expected).ok_or_else(|| {
            RuntimeBindingError::InvalidElementValue {
                element,
                name: name.to_owned(),
                expected,
            }
        })?;
        self.surface
            .invoke_command(node, command, arguments, None)?;
        Ok(WhiskerValue::Null)
    }
}

const EVENT_POINTER: u64 = 1 << 0;
const EVENT_ACTIVATION: u64 = 1 << 1;
const EVENT_NAMED: u64 = 1 << 2;

fn event_class_mask(name: &str) -> u64 {
    match name {
        "touchstart" | "touchmove" | "touchend" | "touchcancel" | "pointerdown" | "pointermove"
        | "pointerup" | "pointercancel" => EVENT_POINTER,
        "tap" | "click" | "longpress" => EVENT_ACTIVATION,
        _ => EVENT_NAMED,
    }
}

fn event_mask(kind: &BoundElementKind, name: &str) -> u64 {
    kind.registration()
        .and_then(|registration| registration.event_named(name))
        .and_then(|event| event.mask())
        .unwrap_or_else(|| event_class_mask(name))
}

fn input_body(event: &InputEvent, target: NodeId) -> WhiskerValue {
    let pointer_kind = event.pointer.map(|pointer| match pointer.kind {
        whisker_engine::whisker_protocol::PointerKind::Mouse => "mouse",
        whisker_engine::whisker_protocol::PointerKind::Touch => "touch",
        whisker_engine::whisker_protocol::PointerKind::Pen => "pen",
        whisker_engine::whisker_protocol::PointerKind::Unknown => "unknown",
    });
    let detail = if let Some(pointer) = event.pointer {
        WhiskerValue::map([
            ("x", WhiskerValue::Float(f64::from(pointer.position.x))),
            ("y", WhiskerValue::Float(f64::from(pointer.position.y))),
        ])
    } else {
        event.detail.clone()
    };
    let mut entries = vec![
        (
            "type",
            WhiskerValue::String(
                event
                    .kind
                    .name(event.pointer.map(|pointer| pointer.kind))
                    .to_owned(),
            ),
        ),
        ("timestamp", WhiskerValue::Float(event.timestamp_ms)),
        (
            "target",
            WhiskerValue::map([("uid", WhiskerValue::Int(target.get() as i64))]),
        ),
        (
            "currentTarget",
            WhiskerValue::map([("uid", WhiskerValue::Int(target.get() as i64))]),
        ),
        ("detail", detail),
    ];
    if let Some(pointer) = event.pointer {
        entries.extend([
            ("pointerId", WhiskerValue::Int(pointer.id.get() as i64)),
            (
                "pointerType",
                WhiskerValue::String(pointer_kind.unwrap_or("unknown").to_owned()),
            ),
            ("buttons", WhiskerValue::Int(i64::from(pointer.buttons))),
            (
                "button",
                WhiskerValue::Int(i64::from(pointer.changed_button)),
            ),
        ]);
    }
    WhiskerValue::map(entries)
}

fn with_current_target(body: &WhiskerValue, target: NodeId) -> WhiskerValue {
    let mut body = body.clone();
    if let WhiskerValue::Map(entries) = &mut body {
        entries.insert(
            "currentTarget".to_owned(),
            WhiskerValue::map([("uid", WhiskerValue::Int(target.get() as i64))]),
        );
    }
    body
}

fn retained_value(value: &WhiskerValue) -> Option<WhiskerValue> {
    Some(match value {
        WhiskerValue::Null => WhiskerValue::Null,
        WhiskerValue::Bool(value) => WhiskerValue::Bool(*value),
        WhiskerValue::Int(value) => WhiskerValue::Int(*value),
        WhiskerValue::Float(value) => WhiskerValue::Float(*value),
        WhiskerValue::String(value) => WhiskerValue::String(value.clone()),
        WhiskerValue::Bytes(value) => WhiskerValue::Bytes(value.clone()),
        WhiskerValue::Array(values) => WhiskerValue::Array(
            values
                .iter()
                .map(retained_value)
                .collect::<Option<Vec<_>>>()?,
        ),
        WhiskerValue::Map(values) => WhiskerValue::Map(
            values
                .iter()
                .map(|(name, value)| Some((name.clone(), retained_value(value)?)))
                .collect::<Option<std::collections::BTreeMap<_, _>>>()?,
        ),
        WhiskerValue::Error(_) => return None,
    })
}

fn command_arguments(value: &WhiskerValue, expected: ElementValueKind) -> Option<WhiskerValue> {
    if expected == ElementValueKind::Null
        && matches!(
            value,
            WhiskerValue::Map(values)
                if matches!(values.get("args"), Some(WhiskerValue::Array(args)) if args.is_empty())
        )
    {
        return Some(WhiskerValue::Null);
    }
    let value = retained_value(value)?;
    expected.accepts(&value).then_some(value)
}

impl DynRenderer for SurfaceRuntime {
    fn create_element(&self, tag: ElementTag) -> Element {
        let mut state = self.state.borrow_mut();
        match state.allocate(tag) {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let mut state = self.state.borrow_mut();
        match state.allocate_named(tag_name) {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn create_element_by_schema(&self, schema: &ElementSchema) -> Element {
        let mut state = self.state.borrow_mut();
        let registration = state
            .registry
            .register_named(schema.clone())
            .map(|_| ())
            .map_err(RuntimeBindingError::from);
        let result = registration.and_then(|()| state.allocate_named(&schema.name));
        match result {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn release_element(&self, handle: Element) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let Some(entry) = state.elements.remove(&handle) else {
                return Ok(());
            };
            if let Some(parent) = entry.parent
                && let Some(parent_entry) = state.elements.get_mut(&parent)
            {
                parent_entry.children.retain(|child| *child != handle);
            }
            if let Some(node) = entry.node {
                state.node_elements.remove(&node);
                if state.surface.node(node).is_some() {
                    let removed_nodes = state.surface_subtree(node);
                    let mut background_resources = state.background_resources.clone();
                    let resource_commands = background_resources.remove_nodes(&removed_nodes);
                    state.surface.delete_node(node)?;
                    state.background_resources = background_resources;
                    state.enqueue_automatic_commands(resource_commands);
                }
            }
            Ok(())
        })();
        state.record(result);
    }

    fn set_attribute(&self, handle: Element, key: &str, value: &str) {
        let mut state = self.state.borrow_mut();
        let result = state.set_attribute(handle, key, value);
        state.record(result);
    }

    fn set_attribute_int(&self, handle: Element, key: &str, value: i64) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Int(value));
        state.record(result);
    }

    fn set_attribute_bool(&self, handle: Element, key: &str, value: bool) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Bool(value));
        state.record(result);
    }

    fn set_attribute_double(&self, handle: Element, key: &str, value: f64) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Float(value));
        state.record(result);
    }

    fn set_inline_styles(&self, handle: Element, css: &str) {
        if css.trim().is_empty() {
            return;
        }
        let mut state = self.state.borrow_mut();
        state.record(Err(RuntimeBindingError::UnsupportedRawStyle {
            element: handle,
            css: css.to_owned(),
        }));
    }

    fn set_specified_style(&self, handle: Element, style: &SpecifiedStyle) -> bool {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let previous = state.element(handle)?.specified.clone();
            state.element_mut(handle)?.specified = style.clone();
            if let Err(error) = state.apply_subtree(handle) {
                state.element_mut(handle)?.specified = previous;
                return Err(error);
            }
            Ok(())
        })();
        let accepted = result.is_ok();
        state.record(result);
        accepted
    }

    fn append_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, None);
        state.record(result);
    }

    fn remove_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.detach(parent, child);
        state.record(result);
    }

    fn supports_insert_before(&self) -> bool {
        true
    }

    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, reference);
        state.record(result);
    }

    fn set_event_listener(
        &self,
        handle: Element,
        event_name: &str,
        bind_type: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let node = state
                .element(handle)?
                .node
                .ok_or(RuntimeBindingError::InvalidRoot { element: handle })?;
            state
                .element_mut(handle)?
                .listeners
                .entry(event_name.to_owned())
                .or_default()
                .push(RuntimeListener {
                    bind_type,
                    callback: Rc::from(callback),
                });
            let entry = state.element(handle)?;
            let mask = entry
                .listeners
                .keys()
                .fold(0, |mask, name| mask | event_mask(&entry.kind, name));
            state.surface.set_event_mask(node, mask)?;
            state.surface.set_hit_test(node, HitTestBehavior::Auto)?;
            Ok(())
        })();
        state.record(result);
    }

    fn invoke_element_method(
        &self,
        handle: Element,
        method: &str,
        params: WhiskerValue,
    ) -> Option<WhiskerValue> {
        let mut state = self.state.borrow_mut();
        Some(match state.invoke_command(handle, method, &params) {
            Ok(value) => {
                state.record(Ok(()));
                value
            }
            Err(error) => {
                let message = error.to_string();
                state.record(Err(error));
                WhiskerValue::Error(message)
            }
        })
    }

    fn set_root(&self, root: Element) {
        let mut state = self.state.borrow_mut();
        let result = match state.element(root) {
            Ok(entry) => match entry.node {
                Some(node) => {
                    state.root = Some(node);
                    Ok(())
                }
                None => Err(RuntimeBindingError::InvalidRoot { element: root }),
            },
            Err(error) => Err(error),
        };
        state.record(result);
    }

    fn flush(&self) {}
}
