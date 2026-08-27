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
    BoxPaint, ElementRegistration, ElementSchema, ElementValueKind, HitTestBehavior, InputEvent,
    InputEventError, MeasurementReady, NodeId, PaintColor, ResourceCommand, ResourceEvent,
    ResourceId, ResourceMessageError, SurfaceId,
};
use whisker_engine::whisker_style::{
    AnimationValue, ComputedFlexBasis, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedLengthPercentageAuto, ComputedSizeValue, ComputedTransformFunction,
    ComputedTransformStyle, ComputedTransitionProperty, InheritedStyle, MotionDirection,
    MotionEasing, MotionFillMode, MotionIterationCount, MotionPlayState, ResolvedNodeStyle,
    SpecifiedStyle, StyleEnvironment, StyleNumber, StyleProperty, StyleResolutionError,
    resolve_style,
};
use whisker_engine::{
    DeferredMeasurementApply, FrameSink, LayoutError, LayoutOptions, LayoutProgress,
    MeasurementProvider, PlainTextInput, SurfaceEngine, SurfaceError, SurfacePresentError,
    lower_color, lower_paint, lower_transform,
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
    /// The Host supplied a non-finite motion timestamp.
    InvalidMotionTimestamp,
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

    /// Samples active Rust-owned transitions at one Host frame timestamp.
    ///
    /// Returns `true` while at least one transition still needs another frame.
    pub fn step_motion(&self, timestamp_ms: f64) -> Result<bool, RuntimeBindingError> {
        if !timestamp_ms.is_finite() {
            return Err(RuntimeBindingError::InvalidMotionTimestamp);
        }
        let mut state = self.state.borrow_mut();
        state.ensure_valid()?;
        let mut completed_layout_transitions = Vec::new();
        for (element, entry) in &mut state.elements {
            for animation in &mut entry.animations {
                animation.sample(timestamp_ms);
            }
            for (property, transition) in entry
                .layout_transitions
                .as_deref_mut()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter_mut())
            {
                let (progress, complete) = transition.sample_progress(timestamp_ms);
                transition.current =
                    interpolate_animated_property(&transition.from, &transition.to, progress);
                if complete {
                    completed_layout_transitions.push((*element, *property));
                }
            }
        }
        {
            let state = &mut *state;
            BindingState::apply_keyframe_animation_values(&state.elements, &mut state.surface)?;
        }
        let mut samples = Vec::new();
        for (element, entry) in &mut state.elements {
            let Some(node) = entry.node else {
                continue;
            };
            let opacity = entry.opacity_transition.as_deref_mut().map(|transition| {
                let (progress, complete) = transition.sample_progress(timestamp_ms);
                transition.current = (transition.from
                    + (transition.to - transition.from) * progress)
                    .clamp(0.0, 1.0);
                (transition.current, complete)
            });
            let colors = entry
                .color_transitions
                .as_deref_mut()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter_mut())
                .map(|(property, transition)| {
                    let (progress, complete) = transition.sample_progress(timestamp_ms);
                    transition.current = transition.from.interpolate(transition.to, progress);
                    (*property, transition.current, complete)
                })
                .collect::<Vec<_>>();
            let text_color = entry
                .text_color_transition
                .as_deref_mut()
                .map(|transition| {
                    let (progress, complete) = transition.sample_progress(timestamp_ms);
                    transition.current = transition.from.interpolate(transition.to, progress);
                    complete
                });
            let transform = entry.transform_transition.as_deref_mut().map(|transition| {
                let (progress, complete) = transition.sample_progress(timestamp_ms);
                transition.current =
                    interpolate_transform_style(&transition.from, &transition.to, progress)
                        .expect("only compatible transform lists enter the active timeline");
                (transition.current.clone(), complete)
            });
            if opacity.is_some()
                || !colors.is_empty()
                || text_color.is_some()
                || transform.is_some()
            {
                samples.push((*element, node, opacity, colors, text_color, transform));
            }
        }
        let mut completed_text_colors = Vec::new();
        for (element, node, opacity, colors, text_color, transform) in samples {
            if let Some((opacity, complete)) = opacity {
                state.surface.set_opacity(node, opacity)?;
                if complete {
                    state.element_mut(element)?.opacity_transition = None;
                }
            }
            if !colors.is_empty() {
                let mut paint = state
                    .surface
                    .node(node)
                    .and_then(|node| node.box_paint())
                    .cloned()
                    .ok_or(RuntimeBindingError::UnknownElement { element })?;
                for (property, color, complete) in colors {
                    set_box_color(&mut paint, property, color.into_paint());
                    if complete {
                        let entry = state.element_mut(element)?;
                        if let Some(transitions) = entry.color_transitions.as_deref_mut() {
                            transitions.0.remove(&property);
                            if transitions.0.is_empty() {
                                entry.color_transitions = None;
                            }
                        }
                    }
                }
                state.surface.set_box_paint(node, paint)?;
            }
            if text_color == Some(true) {
                completed_text_colors.push(element);
            }
            if let Some((mut transform, complete)) = transform {
                if let Some((x, y)) = active_transform_origin(state.element(element)?) {
                    transform.origin_x = x;
                    transform.origin_y = y;
                }
                BindingState::apply_transform_update(node, &transform, &mut state.surface)?;
                if complete {
                    state.element_mut(element)?.transform_transition = None;
                }
            }
        }
        let text_updates = BindingState::active_text_color_updates(&state.elements);
        BindingState::apply_text_color_updates(text_updates, &mut state.surface)?;
        for element in completed_text_colors {
            state.element_mut(element)?.text_color_transition = None;
        }
        for (element, property) in completed_layout_transitions {
            let entry = state.element_mut(element)?;
            if let Some(transitions) = entry.layout_transitions.as_deref_mut() {
                transitions.0.remove(&property);
                if transitions.0.is_empty() {
                    entry.layout_transitions = None;
                }
            }
        }
        Ok(state.elements.values().any(has_active_transition))
    }

    /// Returns whether this surface has an active Rust-owned transition.
    pub fn has_active_motion(&self) -> bool {
        self.state
            .borrow()
            .elements
            .values()
            .any(has_active_transition)
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
        let layout = state
            .surface
            .drive_layout(root, viewport, environment_epoch, provider, options)
            .map_err(RuntimeLayoutError::Measurement)?;
        let transform_updates = BindingState::active_transform_updates(&state.elements);
        BindingState::apply_transform_updates(transform_updates, &mut state.surface)
            .map_err(RuntimeLayoutError::Binding)?;
        Ok(layout)
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
    style_initialized: bool,
    opacity_transition: Option<Box<ActiveTransition<f32>>>,
    color_transitions: Option<Box<ActiveColorTransitions>>,
    text_color_transition: Option<Box<ActiveTransition<RgbaColor>>>,
    transform_transition: Option<Box<ActiveTransition<ComputedTransformStyle>>>,
    layout_transitions: Option<Box<ActivePropertyTransitions>>,
    animations: Vec<ActiveKeyframeAnimation>,
}

#[derive(Clone)]
struct ActiveTransition<Value> {
    from: Value,
    to: Value,
    current: Value,
    duration_ms: f32,
    delay_ms: f32,
    easing: MotionEasing,
    start_ms: Option<f64>,
}

#[derive(Clone)]
struct ActiveColorTransitions(HashMap<StyleProperty, ActiveTransition<RgbaColor>>);

#[derive(Clone)]
struct ActivePropertyTransitions(HashMap<StyleProperty, ActiveTransition<AnimatedPropertyValue>>);

#[derive(Clone, Debug, PartialEq)]
enum AnimatedPropertyValue {
    Number(f32),
    Color(RgbaColor),
    LengthPercentage(ComputedLengthPercentage),
    LengthPercentageAuto(ComputedLengthPercentageAuto),
    Size(ComputedSizeValue),
    FlexBasis(ComputedFlexBasis),
    Transform(ComputedTransformStyle),
    TransformOrigin {
        x: ComputedLengthPercentage,
        y: ComputedLengthPercentage,
    },
}

#[derive(Clone)]
struct KeyframePoint {
    offset: f32,
    value: AnimatedPropertyValue,
    easing: Option<MotionEasing>,
}

#[derive(Clone)]
struct KeyframePropertyTrack {
    property: StyleProperty,
    points: Vec<KeyframePoint>,
}

#[derive(Clone)]
struct ActiveKeyframeAnimation {
    declaration: AnimationValue,
    tracks: Vec<KeyframePropertyTrack>,
    current_time_ms: f64,
    last_timestamp_ms: Option<f64>,
    current: HashMap<StyleProperty, AnimatedPropertyValue>,
    finished: bool,
}

fn has_active_transition(entry: &BoundElement) -> bool {
    entry.opacity_transition.is_some()
        || entry.text_color_transition.is_some()
        || entry.transform_transition.is_some()
        || entry
            .layout_transitions
            .as_deref()
            .is_some_and(|transitions| !transitions.0.is_empty())
        || entry
            .color_transitions
            .as_deref()
            .is_some_and(|transitions| !transitions.0.is_empty())
        || entry
            .animations
            .iter()
            .any(ActiveKeyframeAnimation::needs_frame)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RgbaColor {
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
}

impl<Value> ActiveTransition<Value> {
    fn sample_progress(&mut self, timestamp_ms: f64) -> (f32, bool) {
        let start_ms = *self.start_ms.get_or_insert(timestamp_ms);
        let elapsed_ms = timestamp_ms - start_ms - f64::from(self.delay_ms);
        let linear = (elapsed_ms / f64::from(self.duration_ms)).clamp(0.0, 1.0) as f32;
        (
            self.easing.sample(linear),
            elapsed_ms >= f64::from(self.duration_ms),
        )
    }
}

impl ActiveKeyframeAnimation {
    fn needs_frame(&self) -> bool {
        !self.finished && self.declaration.play_state == MotionPlayState::Running
    }

    fn sample(&mut self, timestamp_ms: f64) {
        if let Some(previous_timestamp_ms) = self.last_timestamp_ms
            && self.declaration.play_state == MotionPlayState::Running
        {
            self.current_time_ms += (timestamp_ms - previous_timestamp_ms).max(0.0);
        }
        self.last_timestamp_ms = Some(timestamp_ms);
        self.sample_current_time();
    }

    fn sample_current_time(&mut self) {
        let local_ms = self.current_time_ms - f64::from(self.declaration.delay.get());
        let duration_ms = f64::from(self.declaration.duration.get());
        let iterations = match self.declaration.iteration_count {
            MotionIterationCount::Infinite => f64::INFINITY,
            MotionIterationCount::Count(value) => f64::from(value.get()),
        };
        let active_duration = if duration_ms == 0.0 || iterations == 0.0 {
            0.0
        } else {
            duration_ms * iterations
        };

        let progress = if local_ms < 0.0 {
            self.finished = false;
            matches!(
                self.declaration.fill_mode,
                MotionFillMode::Backwards | MotionFillMode::Both
            )
            .then(|| directed_iteration_progress(0.0, self.declaration.direction, false))
        } else if local_ms >= active_duration && active_duration.is_finite() {
            self.finished = true;
            matches!(
                self.declaration.fill_mode,
                MotionFillMode::Forwards | MotionFillMode::Both
            )
            .then(|| directed_iteration_progress(iterations, self.declaration.direction, true))
        } else if duration_ms == 0.0 {
            self.finished = true;
            None
        } else {
            self.finished = false;
            Some(directed_iteration_progress(
                local_ms / duration_ms,
                self.declaration.direction,
                false,
            ))
        };

        self.current.clear();
        let Some(progress) = progress else {
            return;
        };
        for track in &self.tracks {
            self.current.insert(
                track.property,
                sample_keyframe_track(track, progress, self.declaration.easing),
            );
        }
    }
}

fn directed_iteration_progress(overall: f64, direction: MotionDirection, at_end: bool) -> f32 {
    let (iteration, progress) = if at_end && overall > 0.0 {
        let ceiling = overall.ceil();
        let fractional = overall - overall.floor();
        if fractional == 0.0 {
            ((ceiling - 1.0) as u64, 1.0)
        } else {
            (overall.floor() as u64, fractional as f32)
        }
    } else {
        (overall.floor() as u64, (overall - overall.floor()) as f32)
    };
    let reverse = match direction {
        MotionDirection::Normal => false,
        MotionDirection::Reverse => true,
        MotionDirection::Alternate => iteration % 2 == 1,
        MotionDirection::AlternateReverse => iteration % 2 == 0,
    };
    if reverse { 1.0 - progress } else { progress }
}

fn sample_keyframe_track(
    track: &KeyframePropertyTrack,
    progress: f32,
    default_easing: MotionEasing,
) -> AnimatedPropertyValue {
    let first = track
        .points
        .first()
        .expect("compiled keyframe tracks are non-empty");
    if progress <= first.offset {
        return first.value.clone();
    }
    let last = track
        .points
        .last()
        .expect("compiled keyframe tracks are non-empty");
    if progress >= last.offset {
        return last.value.clone();
    }
    for points in track.points.windows(2) {
        let from = &points[0];
        let to = &points[1];
        if progress <= to.offset {
            let interval = to.offset - from.offset;
            let local = if interval == 0.0 {
                1.0
            } else {
                (progress - from.offset) / interval
            };
            let eased = from.easing.unwrap_or(default_easing).sample(local);
            return interpolate_animated_property(&from.value, &to.value, eased);
        }
    }
    last.value.clone()
}

fn interpolate_animated_property(
    from: &AnimatedPropertyValue,
    to: &AnimatedPropertyValue,
    progress: f32,
) -> AnimatedPropertyValue {
    match (from, to) {
        (AnimatedPropertyValue::Number(from), AnimatedPropertyValue::Number(to)) => {
            AnimatedPropertyValue::Number(from + (to - from) * progress)
        }
        (AnimatedPropertyValue::Color(from), AnimatedPropertyValue::Color(to)) => {
            AnimatedPropertyValue::Color(from.interpolate(*to, progress))
        }
        (
            AnimatedPropertyValue::LengthPercentage(from),
            AnimatedPropertyValue::LengthPercentage(to),
        ) => AnimatedPropertyValue::LengthPercentage(interpolate_length_percentage(
            *from, *to, progress,
        )),
        (
            AnimatedPropertyValue::LengthPercentageAuto(from),
            AnimatedPropertyValue::LengthPercentageAuto(to),
        ) => AnimatedPropertyValue::LengthPercentageAuto(match (from, to) {
            (
                ComputedLengthPercentageAuto::Value(from),
                ComputedLengthPercentageAuto::Value(to),
            ) => ComputedLengthPercentageAuto::Value(interpolate_length_percentage(
                *from, *to, progress,
            )),
            _ if progress < 0.5 => *from,
            _ => *to,
        }),
        (AnimatedPropertyValue::Size(from), AnimatedPropertyValue::Size(to)) => {
            AnimatedPropertyValue::Size(match (from, to) {
                (ComputedSizeValue::Value(from), ComputedSizeValue::Value(to)) => {
                    ComputedSizeValue::Value(interpolate_length_percentage(*from, *to, progress))
                }
                _ if progress < 0.5 => *from,
                _ => *to,
            })
        }
        (AnimatedPropertyValue::FlexBasis(from), AnimatedPropertyValue::FlexBasis(to)) => {
            AnimatedPropertyValue::FlexBasis(match (from, to) {
                (ComputedFlexBasis::Value(from), ComputedFlexBasis::Value(to)) => {
                    ComputedFlexBasis::Value(interpolate_length_percentage(*from, *to, progress))
                }
                _ if progress < 0.5 => *from,
                _ => *to,
            })
        }
        (AnimatedPropertyValue::Transform(from), AnimatedPropertyValue::Transform(to)) => {
            interpolate_transform_style(from, to, progress).map_or_else(
                || {
                    AnimatedPropertyValue::Transform(if progress < 0.5 {
                        from.clone()
                    } else {
                        to.clone()
                    })
                },
                AnimatedPropertyValue::Transform,
            )
        }
        (
            AnimatedPropertyValue::TransformOrigin {
                x: from_x,
                y: from_y,
            },
            AnimatedPropertyValue::TransformOrigin { x: to_x, y: to_y },
        ) => AnimatedPropertyValue::TransformOrigin {
            x: interpolate_length_percentage(*from_x, *to_x, progress),
            y: interpolate_length_percentage(*from_y, *to_y, progress),
        },
        _ if progress < 0.5 => from.clone(),
        _ => to.clone(),
    }
}

fn interpolate_length_percentage(
    from: ComputedLengthPercentage,
    to: ComputedLengthPercentage,
    progress: f32,
) -> ComputedLengthPercentage {
    ComputedLengthPercentage::new(
        from.length() + (to.length() - from.length()) * progress,
        from.fraction() + (to.fraction() - from.fraction()) * progress,
    )
}

fn interpolate_transform_style(
    from: &ComputedTransformStyle,
    to: &ComputedTransformStyle,
    progress: f32,
) -> Option<ComputedTransformStyle> {
    let count = from.functions.len().max(to.functions.len());
    let mut functions = Vec::with_capacity(count);
    for index in 0..count {
        let function = match (from.functions.get(index), to.functions.get(index)) {
            (Some(from), Some(to)) => interpolate_transform_function(from, to, progress)?,
            (Some(from), None) => {
                interpolate_transform_function(from, &identity_transform_function(from)?, progress)?
            }
            (None, Some(to)) => {
                interpolate_transform_function(&identity_transform_function(to)?, to, progress)?
            }
            (None, None) => unreachable!("index is bounded by the longest transform list"),
        };
        functions.push(function);
    }
    let mut current = to.clone();
    current.functions = functions;
    Some(current)
}

fn interpolate_transform_function(
    from: &ComputedTransformFunction,
    to: &ComputedTransformFunction,
    progress: f32,
) -> Option<ComputedTransformFunction> {
    let number = |from: StyleNumber, to: StyleNumber| {
        StyleNumber::new(from.get() + (to.get() - from.get()) * progress)
    };
    let length = |from: ComputedLengthPercentage, to: ComputedLengthPercentage| {
        ComputedLengthPercentage::new(
            from.length() + (to.length() - from.length()) * progress,
            from.fraction() + (to.fraction() - from.fraction()) * progress,
        )
    };
    match (from, to) {
        (
            ComputedTransformFunction::Translate {
                x: from_x,
                y: from_y,
                z: from_z,
            },
            ComputedTransformFunction::Translate {
                x: to_x,
                y: to_y,
                z: to_z,
            },
        ) => Some(ComputedTransformFunction::Translate {
            x: length(*from_x, *to_x),
            y: length(*from_y, *to_y),
            z: number(*from_z, *to_z),
        }),
        (ComputedTransformFunction::RotateX(from), ComputedTransformFunction::RotateX(to)) => {
            Some(ComputedTransformFunction::RotateX(number(*from, *to)))
        }
        (ComputedTransformFunction::RotateY(from), ComputedTransformFunction::RotateY(to)) => {
            Some(ComputedTransformFunction::RotateY(number(*from, *to)))
        }
        (ComputedTransformFunction::RotateZ(from), ComputedTransformFunction::RotateZ(to)) => {
            Some(ComputedTransformFunction::RotateZ(number(*from, *to)))
        }
        (
            ComputedTransformFunction::Scale {
                x: from_x,
                y: from_y,
                z: from_z,
            },
            ComputedTransformFunction::Scale {
                x: to_x,
                y: to_y,
                z: to_z,
            },
        ) => Some(ComputedTransformFunction::Scale {
            x: number(*from_x, *to_x),
            y: number(*from_y, *to_y),
            z: number(*from_z, *to_z),
        }),
        (
            ComputedTransformFunction::Skew {
                x_degrees: from_x,
                y_degrees: from_y,
            },
            ComputedTransformFunction::Skew {
                x_degrees: to_x,
                y_degrees: to_y,
            },
        ) => Some(ComputedTransformFunction::Skew {
            x_degrees: number(*from_x, *to_x),
            y_degrees: number(*from_y, *to_y),
        }),
        // CSS matrix pairs require decomposition; they intentionally snap in
        // this work unit instead of using visually incorrect element-wise
        // interpolation.
        _ => None,
    }
}

fn identity_transform_function(
    function: &ComputedTransformFunction,
) -> Option<ComputedTransformFunction> {
    let zero = StyleNumber::new(0.0);
    let one = StyleNumber::new(1.0);
    match function {
        ComputedTransformFunction::Translate { .. } => Some(ComputedTransformFunction::Translate {
            x: ComputedLengthPercentage::ZERO,
            y: ComputedLengthPercentage::ZERO,
            z: zero,
        }),
        ComputedTransformFunction::RotateX(_) => Some(ComputedTransformFunction::RotateX(zero)),
        ComputedTransformFunction::RotateY(_) => Some(ComputedTransformFunction::RotateY(zero)),
        ComputedTransformFunction::RotateZ(_) => Some(ComputedTransformFunction::RotateZ(zero)),
        ComputedTransformFunction::Scale { .. } => Some(ComputedTransformFunction::Scale {
            x: one,
            y: one,
            z: one,
        }),
        ComputedTransformFunction::Skew { .. } => Some(ComputedTransformFunction::Skew {
            x_degrees: zero,
            y_degrees: zero,
        }),
        ComputedTransformFunction::Matrix(_) => None,
    }
}

impl RgbaColor {
    fn from_paint(value: &PaintColor) -> Option<Self> {
        match value {
            PaintColor::Srgba {
                red,
                green,
                blue,
                alpha,
            } => Some(Self {
                red: f32::from(*red) / 255.0,
                green: f32::from(*green) / 255.0,
                blue: f32::from(*blue) / 255.0,
                alpha: *alpha,
            }),
            PaintColor::Hsla {
                hue_degrees,
                saturation,
                lightness,
                alpha,
            } => {
                let hue = hue_degrees.rem_euclid(360.0) / 360.0;
                let saturation = saturation / 100.0;
                let lightness = lightness / 100.0;
                let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
                let sector = hue * 6.0;
                let intermediate = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
                let (red, green, blue) = match sector.floor() as u8 {
                    0 => (chroma, intermediate, 0.0),
                    1 => (intermediate, chroma, 0.0),
                    2 => (0.0, chroma, intermediate),
                    3 => (0.0, intermediate, chroma),
                    4 => (intermediate, 0.0, chroma),
                    _ => (chroma, 0.0, intermediate),
                };
                let offset = lightness - chroma * 0.5;
                Some(Self {
                    red: red + offset,
                    green: green + offset,
                    blue: blue + offset,
                    alpha: *alpha,
                })
            }
            PaintColor::Named(name) if name.eq_ignore_ascii_case("transparent") => Some(Self {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 0.0,
            }),
            PaintColor::Named(_) => None,
        }
    }

    fn interpolate(self, target: Self, progress: f32) -> Self {
        let mix = |from: f32, to: f32| from + (to - from) * progress;
        let alpha = mix(self.alpha, target.alpha).clamp(0.0, 1.0);
        let channel = |from: f32, to: f32| {
            if alpha == 0.0 {
                0.0
            } else {
                (mix(from * self.alpha, to * target.alpha) / alpha).clamp(0.0, 1.0)
            }
        };
        Self {
            red: channel(self.red, target.red),
            green: channel(self.green, target.green),
            blue: channel(self.blue, target.blue),
            alpha,
        }
    }

    fn into_paint(self) -> PaintColor {
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        PaintColor::Srgba {
            red: channel(self.red),
            green: channel(self.green),
            blue: channel(self.blue),
            alpha: self.alpha.clamp(0.0, 1.0),
        }
    }
}

const BOX_COLOR_PROPERTIES: [StyleProperty; 5] = [
    StyleProperty::BackgroundColor,
    StyleProperty::BorderTopColor,
    StyleProperty::BorderRightColor,
    StyleProperty::BorderBottomColor,
    StyleProperty::BorderLeftColor,
];

fn box_color(paint: &BoxPaint, property: StyleProperty) -> &PaintColor {
    match property {
        StyleProperty::BackgroundColor => &paint.background_color,
        StyleProperty::BorderTopColor => &paint.border_colors.top,
        StyleProperty::BorderRightColor => &paint.border_colors.right,
        StyleProperty::BorderBottomColor => &paint.border_colors.bottom,
        StyleProperty::BorderLeftColor => &paint.border_colors.left,
        _ => unreachable!("only box color properties enter the transition table"),
    }
}

fn set_box_color(paint: &mut BoxPaint, property: StyleProperty, color: PaintColor) {
    match property {
        StyleProperty::BackgroundColor => paint.background_color = color,
        StyleProperty::BorderTopColor => paint.border_colors.top = color,
        StyleProperty::BorderRightColor => paint.border_colors.right = color,
        StyleProperty::BorderBottomColor => paint.border_colors.bottom = color,
        StyleProperty::BorderLeftColor => paint.border_colors.left = color,
        _ => unreachable!("only box color properties enter the transition table"),
    }
}

const LAYOUT_ANIMATED_PROPERTIES: [StyleProperty; 23] = [
    StyleProperty::Left,
    StyleProperty::Right,
    StyleProperty::Top,
    StyleProperty::Bottom,
    StyleProperty::Width,
    StyleProperty::Height,
    StyleProperty::MinWidth,
    StyleProperty::MinHeight,
    StyleProperty::MaxWidth,
    StyleProperty::MaxHeight,
    StyleProperty::MarginTop,
    StyleProperty::MarginRight,
    StyleProperty::MarginBottom,
    StyleProperty::MarginLeft,
    StyleProperty::PaddingTop,
    StyleProperty::PaddingRight,
    StyleProperty::PaddingBottom,
    StyleProperty::PaddingLeft,
    StyleProperty::BorderTopWidth,
    StyleProperty::BorderRightWidth,
    StyleProperty::BorderBottomWidth,
    StyleProperty::BorderLeftWidth,
    StyleProperty::FlexBasis,
];

fn keyframe_property(property: StyleProperty) -> bool {
    LAYOUT_ANIMATED_PROPERTIES.contains(&property)
        || matches!(
            property,
            StyleProperty::FlexGrow
                | StyleProperty::Opacity
                | StyleProperty::BackgroundColor
                | StyleProperty::BorderTopColor
                | StyleProperty::BorderRightColor
                | StyleProperty::BorderBottomColor
                | StyleProperty::BorderLeftColor
                | StyleProperty::Color
                | StyleProperty::Transform
                | StyleProperty::TransformOrigin
        )
}

fn animated_property_value(
    resolved: &ResolvedNodeStyle,
    property: StyleProperty,
) -> Option<AnimatedPropertyValue> {
    let computed = resolved.computed();
    let layout = computed.layout();
    match property {
        StyleProperty::Left => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.left,
        )),
        StyleProperty::Right => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.right,
        )),
        StyleProperty::Top => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.top,
        )),
        StyleProperty::Bottom => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.inset.bottom,
        )),
        StyleProperty::Width => Some(AnimatedPropertyValue::Size(layout.size.width)),
        StyleProperty::Height => Some(AnimatedPropertyValue::Size(layout.size.height)),
        StyleProperty::MinWidth => Some(AnimatedPropertyValue::Size(layout.min_size.width)),
        StyleProperty::MinHeight => Some(AnimatedPropertyValue::Size(layout.min_size.height)),
        StyleProperty::MaxWidth => Some(AnimatedPropertyValue::Size(layout.max_size.width)),
        StyleProperty::MaxHeight => Some(AnimatedPropertyValue::Size(layout.max_size.height)),
        StyleProperty::MarginTop => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.top,
        )),
        StyleProperty::MarginRight => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.right,
        )),
        StyleProperty::MarginBottom => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.bottom,
        )),
        StyleProperty::MarginLeft => Some(AnimatedPropertyValue::LengthPercentageAuto(
            layout.margin.left,
        )),
        StyleProperty::PaddingTop => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.padding.top))
        }
        StyleProperty::PaddingRight => Some(AnimatedPropertyValue::LengthPercentage(
            layout.padding.right,
        )),
        StyleProperty::PaddingBottom => Some(AnimatedPropertyValue::LengthPercentage(
            layout.padding.bottom,
        )),
        StyleProperty::PaddingLeft => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.padding.left))
        }
        StyleProperty::BorderTopWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.top))
        }
        StyleProperty::BorderRightWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.right))
        }
        StyleProperty::BorderBottomWidth => Some(AnimatedPropertyValue::LengthPercentage(
            layout.border.bottom,
        )),
        StyleProperty::BorderLeftWidth => {
            Some(AnimatedPropertyValue::LengthPercentage(layout.border.left))
        }
        StyleProperty::FlexBasis => Some(AnimatedPropertyValue::FlexBasis(layout.flex_basis)),
        StyleProperty::FlexGrow => Some(AnimatedPropertyValue::Number(layout.flex_grow.get())),
        StyleProperty::Opacity => Some(AnimatedPropertyValue::Number(
            computed.paint().opacity.get(),
        )),
        StyleProperty::Color => {
            RgbaColor::from_paint(&lower_color(computed.inherited_text().color()))
                .map(AnimatedPropertyValue::Color)
        }
        StyleProperty::Transform => Some(AnimatedPropertyValue::Transform(
            computed.paint().transform.clone(),
        )),
        StyleProperty::TransformOrigin => Some(AnimatedPropertyValue::TransformOrigin {
            x: computed.paint().transform.origin_x,
            y: computed.paint().transform.origin_y,
        }),
        property if BOX_COLOR_PROPERTIES.contains(&property) => {
            let paint = lower_paint(computed.paint(), computed.layout()).box_paint;
            RgbaColor::from_paint(box_color(&paint, property)).map(AnimatedPropertyValue::Color)
        }
        _ => None,
    }
}

fn set_animated_layout_property(
    layout: &mut ComputedLayoutStyle,
    property: StyleProperty,
    value: &AnimatedPropertyValue,
) -> bool {
    match (property, value) {
        (StyleProperty::Left, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.left = *value;
        }
        (StyleProperty::Right, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.right = *value;
        }
        (StyleProperty::Top, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.top = *value;
        }
        (StyleProperty::Bottom, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.inset.bottom = *value;
        }
        (StyleProperty::Width, AnimatedPropertyValue::Size(value)) => layout.size.width = *value,
        (StyleProperty::Height, AnimatedPropertyValue::Size(value)) => layout.size.height = *value,
        (StyleProperty::MinWidth, AnimatedPropertyValue::Size(value)) => {
            layout.min_size.width = *value;
        }
        (StyleProperty::MinHeight, AnimatedPropertyValue::Size(value)) => {
            layout.min_size.height = *value;
        }
        (StyleProperty::MaxWidth, AnimatedPropertyValue::Size(value)) => {
            layout.max_size.width = *value;
        }
        (StyleProperty::MaxHeight, AnimatedPropertyValue::Size(value)) => {
            layout.max_size.height = *value;
        }
        (StyleProperty::MarginTop, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.top = *value;
        }
        (StyleProperty::MarginRight, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.right = *value;
        }
        (StyleProperty::MarginBottom, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.bottom = *value;
        }
        (StyleProperty::MarginLeft, AnimatedPropertyValue::LengthPercentageAuto(value)) => {
            layout.margin.left = *value;
        }
        (StyleProperty::PaddingTop, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.top = *value;
        }
        (StyleProperty::PaddingRight, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.right = *value;
        }
        (StyleProperty::PaddingBottom, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.bottom = *value;
        }
        (StyleProperty::PaddingLeft, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.padding.left = *value;
        }
        (StyleProperty::BorderTopWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.top = *value;
        }
        (StyleProperty::BorderRightWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.right = *value;
        }
        (StyleProperty::BorderBottomWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.bottom = *value;
        }
        (StyleProperty::BorderLeftWidth, AnimatedPropertyValue::LengthPercentage(value)) => {
            layout.border.left = *value;
        }
        (StyleProperty::FlexBasis, AnimatedPropertyValue::FlexBasis(value)) => {
            layout.flex_basis = *value;
        }
        (StyleProperty::FlexGrow, AnimatedPropertyValue::Number(value)) => {
            layout.flex_grow = StyleNumber::new(*value);
        }
        _ => return false,
    }
    true
}

fn layout_animation_values(
    resolved: &ResolvedNodeStyle,
) -> HashMap<StyleProperty, AnimatedPropertyValue> {
    LAYOUT_ANIMATED_PROPERTIES
        .into_iter()
        .chain([StyleProperty::FlexGrow, StyleProperty::TransformOrigin])
        .filter_map(|property| {
            animated_property_value(resolved, property).map(|value| (property, value))
        })
        .collect()
}

fn smoothly_interpolable(from: &AnimatedPropertyValue, to: &AnimatedPropertyValue) -> bool {
    matches!(
        (from, to),
        (
            AnimatedPropertyValue::Number(_),
            AnimatedPropertyValue::Number(_)
        ) | (
            AnimatedPropertyValue::Color(_),
            AnimatedPropertyValue::Color(_)
        ) | (
            AnimatedPropertyValue::LengthPercentage(_),
            AnimatedPropertyValue::LengthPercentage(_)
        ) | (
            AnimatedPropertyValue::LengthPercentageAuto(ComputedLengthPercentageAuto::Value(_)),
            AnimatedPropertyValue::LengthPercentageAuto(ComputedLengthPercentageAuto::Value(_))
        ) | (
            AnimatedPropertyValue::Size(ComputedSizeValue::Value(_)),
            AnimatedPropertyValue::Size(ComputedSizeValue::Value(_))
        ) | (
            AnimatedPropertyValue::FlexBasis(ComputedFlexBasis::Value(_)),
            AnimatedPropertyValue::FlexBasis(ComputedFlexBasis::Value(_))
        ) | (
            AnimatedPropertyValue::TransformOrigin { .. },
            AnimatedPropertyValue::TransformOrigin { .. }
        )
    )
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

fn active_transform_origin(
    entry: &BoundElement,
) -> Option<(ComputedLengthPercentage, ComputedLengthPercentage)> {
    let keyframe_origin = entry.animations.iter().rev().find_map(|animation| {
        match animation.current.get(&StyleProperty::TransformOrigin) {
            Some(AnimatedPropertyValue::TransformOrigin { x, y }) => Some((*x, *y)),
            _ => None,
        }
    });
    entry
        .layout_transitions
        .as_deref()
        .and_then(|transitions| transitions.0.get(&StyleProperty::TransformOrigin))
        .and_then(|transition| match &transition.current {
            AnimatedPropertyValue::TransformOrigin { x, y } => Some((*x, *y)),
            _ => None,
        })
        .or(keyframe_origin)
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
        Self::reapply_active_transitions(&self.elements, &mut surface)?;

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
                style_initialized: false,
                opacity_transition: None,
                color_transitions: None,
                text_color_transition: None,
                transform_transition: None,
                layout_transitions: None,
                animations: Vec::new(),
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
        Self::reapply_active_transitions(&self.elements, &mut surface)?;

        self.surface = surface;
        self.background_resources = background_resources;
        for (element, resolved) in updates {
            self.element_mut(element)?.resolved = Some(resolved);
        }
        self.enqueue_automatic_commands(resource_commands);
        Ok(())
    }

    fn reapply_active_transitions(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        Self::apply_keyframe_animation_values(elements, surface)?;
        for (element, entry) in elements {
            let Some(node) = entry.node else {
                continue;
            };
            if let Some(transition) = entry.opacity_transition.as_deref() {
                surface.set_opacity(node, transition.current)?;
            }
            if let Some(transitions) = entry.color_transitions.as_deref() {
                let mut paint = surface
                    .node(node)
                    .and_then(|node| node.box_paint())
                    .cloned()
                    .ok_or(RuntimeBindingError::UnknownElement { element: *element })?;
                for (property, transition) in &transitions.0 {
                    set_box_color(&mut paint, *property, transition.current.into_paint());
                }
                surface.set_box_paint(node, paint)?;
            }
            if let Some(transition) = entry.transform_transition.as_deref() {
                let mut transform = transition.current.clone();
                if let Some((x, y)) = active_transform_origin(entry) {
                    transform.origin_x = x;
                    transform.origin_y = y;
                }
                Self::apply_transform_update(node, &transform, surface)?;
            }
        }
        Self::reapply_active_text_colors(elements, surface)?;
        Ok(())
    }

    fn reapply_active_text_colors(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        Self::apply_text_color_updates(Self::active_text_color_updates(elements), surface)
    }

    fn active_text_color_updates(
        elements: &HashMap<Element, BoundElement>,
    ) -> Vec<(Element, NodeId, RgbaColor)> {
        elements
            .iter()
            .filter_map(|(element, entry)| {
                let node = entry.node?;
                entry.text.as_ref()?;
                Self::active_text_color(*element, elements).map(|color| (*element, node, color))
            })
            .collect()
    }

    fn apply_text_color_updates(
        updates: Vec<(Element, NodeId, RgbaColor)>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (element, node, color) in updates {
            let mut content = surface
                .node(node)
                .and_then(|node| node.text())
                .cloned()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            content.paint.foreground = color.into_paint();
            surface.set_text_content(node, content)?;
        }
        Ok(())
    }

    fn active_text_color(
        element: Element,
        elements: &HashMap<Element, BoundElement>,
    ) -> Option<RgbaColor> {
        let mut current = Some(element);
        while let Some(candidate) = current {
            let entry = elements.get(&candidate)?;
            if let Some(transition) = entry.text_color_transition.as_deref() {
                return Some(transition.current);
            }
            if let Some(color) =
                entry.animations.iter().rev().find_map(|animation| {
                    match animation.current.get(&StyleProperty::Color) {
                        Some(AnimatedPropertyValue::Color(color)) => Some(*color),
                        _ => None,
                    }
                })
            {
                return Some(color);
            }
            if entry
                .specified
                .declarations()
                .any(|declaration| declaration.property() == StyleProperty::Color)
            {
                return None;
            }
            current = entry.parent;
        }
        None
    }

    fn active_transform_updates(
        elements: &HashMap<Element, BoundElement>,
    ) -> Vec<(NodeId, ComputedTransformStyle)> {
        elements
            .values()
            .filter_map(|entry| {
                Some((
                    entry.node?,
                    entry.transform_transition.as_deref()?.current.clone(),
                ))
            })
            .collect()
    }

    fn apply_transform_updates(
        updates: Vec<(NodeId, ComputedTransformStyle)>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (node, transform) in updates {
            Self::apply_transform_update(node, &transform, surface)?;
        }
        Ok(())
    }

    fn apply_transform_update(
        node: NodeId,
        transform: &ComputedTransformStyle,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        let Some(layout) = surface.node(node).and_then(|node| node.layout()) else {
            return Ok(());
        };
        let transform =
            lower_transform(transform, layout.border_box.width, layout.border_box.height)
                .expect("resolved transform and layout geometry must produce a finite matrix");
        surface.set_transform(node, transform)?;
        Ok(())
    }

    fn compile_keyframe_animations(
        &self,
        element: Element,
    ) -> Result<Vec<ActiveKeyframeAnimation>, RuntimeBindingError> {
        let entry = self.element(element)?;
        let base = entry.effective_specified();
        let resolved = entry
            .resolved
            .as_ref()
            .ok_or(RuntimeBindingError::UnknownElement { element })?;
        let parent_inherited = entry
            .parent
            .and_then(|parent| self.elements.get(&parent))
            .and_then(|parent| parent.resolved.as_ref())
            .map(|parent| parent.inherited_for_children().clone());
        let mut animations = Vec::new();
        for declaration in &resolved.computed().motion().animations {
            let Some(keyframes) = declaration.keyframes.as_ref() else {
                continue;
            };
            let properties = keyframes
                .frames
                .iter()
                .flat_map(|frame| frame.style.resolved())
                .map(|declaration| declaration.property())
                .filter(|property| keyframe_property(*property))
                .collect::<HashSet<_>>();
            let mut tracks = Vec::new();
            for property in properties {
                let Some(underlying) = animated_property_value(resolved, property) else {
                    continue;
                };
                let mut points = Vec::new();
                for frame in &keyframes.frames {
                    if !frame
                        .style
                        .resolved()
                        .iter()
                        .any(|declaration| declaration.property() == property)
                    {
                        continue;
                    }
                    let frame_style = base.clone().merge(frame.style.clone());
                    let frame_resolved =
                        resolve_style(&frame_style, parent_inherited.as_ref(), self.environment)?;
                    if let Some(value) = animated_property_value(&frame_resolved, property) {
                        points.push(KeyframePoint {
                            offset: frame.offset.get(),
                            value,
                            easing: frame.easing,
                        });
                    }
                }
                if points.first().is_none_or(|point| point.offset > 0.0) {
                    points.insert(
                        0,
                        KeyframePoint {
                            offset: 0.0,
                            value: underlying.clone(),
                            easing: None,
                        },
                    );
                }
                if points.last().is_none_or(|point| point.offset < 1.0) {
                    points.push(KeyframePoint {
                        offset: 1.0,
                        value: underlying,
                        easing: None,
                    });
                }
                if !points.is_empty() {
                    tracks.push(KeyframePropertyTrack { property, points });
                }
            }
            animations.push(ActiveKeyframeAnimation {
                declaration: declaration.clone(),
                tracks,
                current_time_ms: 0.0,
                last_timestamp_ms: None,
                current: HashMap::new(),
                finished: false,
            });
        }
        for animation in &mut animations {
            animation.sample_current_time();
        }
        Ok(animations)
    }

    fn configure_keyframe_animations(
        &mut self,
        element: Element,
    ) -> Result<(), RuntimeBindingError> {
        let mut animations = self.compile_keyframe_animations(element)?;
        let previous_animations = self.element(element)?.animations.clone();
        for (animation, previous_animation) in animations.iter_mut().zip(previous_animations.iter())
        {
            let same_definition = animation.declaration.name == previous_animation.declaration.name
                && animation.declaration.keyframes == previous_animation.declaration.keyframes;
            if !same_definition {
                continue;
            }
            animation.current_time_ms = previous_animation.current_time_ms;
            animation.last_timestamp_ms = if previous_animation.declaration.play_state
                == MotionPlayState::Paused
                && animation.declaration.play_state == MotionPlayState::Running
            {
                None
            } else {
                previous_animation.last_timestamp_ms
            };
            animation.sample_current_time();
        }
        let needs_frame = animations.iter().any(ActiveKeyframeAnimation::needs_frame);
        self.element_mut(element)?.animations = animations;
        if needs_frame {
            crate::runtime::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    fn apply_keyframe_animation_values(
        elements: &HashMap<Element, BoundElement>,
        surface: &mut SurfaceEngine,
    ) -> Result<(), RuntimeBindingError> {
        for (element, entry) in elements {
            if entry.animations.is_empty() && entry.layout_transitions.is_none() {
                continue;
            }
            let Some(node) = entry.node else {
                continue;
            };
            let Some(resolved) = entry.resolved.as_ref() else {
                continue;
            };
            let mut values = HashMap::new();
            let mut tracked = HashSet::new();
            for animation in &entry.animations {
                tracked.extend(animation.tracks.iter().map(|track| track.property));
                for (property, value) in &animation.current {
                    values.insert(*property, value.clone());
                }
            }
            if let Some(transitions) = entry.layout_transitions.as_deref() {
                tracked.extend(transitions.0.keys().copied());
                for (property, transition) in &transitions.0 {
                    values.insert(*property, transition.current.clone());
                }
            }
            let computed = resolved.computed();
            let mut layout = computed.layout().clone();
            let mut layout_changed = false;
            for (property, value) in &values {
                layout_changed |= set_animated_layout_property(&mut layout, *property, value);
            }
            if layout_changed {
                surface.update_layout_style(node, layout.clone())?;
            }
            if tracked.contains(&StyleProperty::Opacity) {
                let opacity = match values.get(&StyleProperty::Opacity) {
                    Some(AnimatedPropertyValue::Number(value)) => *value,
                    _ => computed.paint().opacity.get(),
                };
                surface.set_opacity(node, opacity.clamp(0.0, 1.0))?;
            }
            if layout_changed
                || tracked
                    .iter()
                    .any(|property| BOX_COLOR_PROPERTIES.contains(property))
            {
                let mut paint = lower_paint(computed.paint(), &layout).box_paint;
                for property in BOX_COLOR_PROPERTIES {
                    if let Some(AnimatedPropertyValue::Color(value)) = values.get(&property) {
                        set_box_color(&mut paint, property, value.into_paint());
                    }
                }
                surface.set_box_paint(node, paint)?;
            }
            if tracked.contains(&StyleProperty::Transform)
                || tracked.contains(&StyleProperty::TransformOrigin)
            {
                let mut transform = match values.get(&StyleProperty::Transform) {
                    Some(AnimatedPropertyValue::Transform(value)) => value.clone(),
                    _ => computed.paint().transform.clone(),
                };
                if let Some(AnimatedPropertyValue::TransformOrigin { x, y }) =
                    values.get(&StyleProperty::TransformOrigin)
                {
                    transform.origin_x = *x;
                    transform.origin_y = *y;
                }
                Self::apply_transform_update(node, &transform, surface)?;
            }
            let _ = element;
        }
        let text_updates = Self::active_text_color_updates(elements);
        Self::apply_text_color_updates(text_updates, surface)
    }

    fn configure_layout_transitions(
        &mut self,
        element: Element,
        previous_targets: &HashMap<StyleProperty, AnimatedPropertyValue>,
        previous_current: &HashMap<StyleProperty, AnimatedPropertyValue>,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (targets, transitions) = {
            let entry = self.element(element)?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            (
                layout_animation_values(resolved),
                resolved.computed().motion().transitions.clone(),
            )
        };
        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.layout_transitions = None;
            return Ok(());
        }

        let mut started = false;
        for (property, target) in targets {
            let Some(previous_target) = previous_targets.get(&property) else {
                continue;
            };
            if previous_target == &target {
                continue;
            }
            if let Some(active) = entry.layout_transitions.as_deref_mut() {
                active.0.remove(&property);
            }
            let transition = transitions.iter().rev().find(|transition| {
                matches!(transition.property, ComputedTransitionProperty::All)
                    || transition.property == ComputedTransitionProperty::Property(property)
            });
            let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
                continue;
            };
            let from = previous_current
                .get(&property)
                .unwrap_or(previous_target)
                .clone();
            if !smoothly_interpolable(&from, &target) {
                continue;
            }
            entry
                .layout_transitions
                .get_or_insert_with(|| Box::new(ActivePropertyTransitions(HashMap::new())))
                .0
                .insert(
                    property,
                    ActiveTransition {
                        from: from.clone(),
                        to: target,
                        current: from,
                        duration_ms: transition.duration.get(),
                        delay_ms: transition.delay.get(),
                        easing: transition.easing,
                        start_ms: None,
                    },
                );
            started = true;
        }
        if entry
            .layout_transitions
            .as_deref()
            .is_some_and(|transitions| transitions.0.is_empty())
        {
            entry.layout_transitions = None;
        }
        if started {
            crate::runtime::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    fn configure_opacity_transition(
        &mut self,
        element: Element,
        previous_target: f32,
        previous_current: f32,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transition) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target = resolved.computed().paint().opacity.get();
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(
                        transition.property,
                        ComputedTransitionProperty::All
                            | ComputedTransitionProperty::Property(StyleProperty::Opacity)
                    )
                })
                .copied();
            (node, target, transition)
        };

        let entry = self.element_mut(element)?;
        entry.style_initialized = true;
        if !was_initialized {
            entry.opacity_transition = None;
            self.surface.set_opacity(node, target)?;
            return Ok(());
        }
        if previous_target.to_bits() == target.to_bits() {
            return Ok(());
        }
        let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
            entry.opacity_transition = None;
            self.surface.set_opacity(node, target)?;
            return Ok(());
        };
        entry.opacity_transition = Some(Box::new(ActiveTransition {
            from: previous_current,
            to: target,
            current: previous_current,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
        }));
        self.surface.set_opacity(node, previous_current)?;
        crate::runtime::runtime_wake::wake_runtime();
        Ok(())
    }

    fn configure_color_transitions(
        &mut self,
        element: Element,
        previous: &BoxPaint,
        previous_current: &HashMap<StyleProperty, RgbaColor>,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transitions) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let computed = resolved.computed();
            (
                node,
                lower_paint(computed.paint(), computed.layout()).box_paint,
                computed.motion().transitions.clone(),
            )
        };

        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.color_transitions = None;
            self.surface.set_box_paint(node, target)?;
            return Ok(());
        }
        let mut started = false;
        for property in BOX_COLOR_PROPERTIES {
            let previous_target = box_color(previous, property);
            let target_color = box_color(&target, property);
            if previous_target == target_color {
                continue;
            }
            if let Some(active) = entry.color_transitions.as_deref_mut() {
                active.0.remove(&property);
            }
            let transition = transitions.iter().rev().find(|transition| {
                matches!(transition.property, ComputedTransitionProperty::All)
                    || transition.property == ComputedTransitionProperty::Property(property)
            });
            let Some(transition) = transition.filter(|value| value.duration.get() > 0.0) else {
                continue;
            };
            let from = previous_current
                .get(&property)
                .copied()
                .or_else(|| RgbaColor::from_paint(previous_target));
            let to = RgbaColor::from_paint(target_color);
            let (Some(from), Some(to)) = (from, to) else {
                continue;
            };
            entry
                .color_transitions
                .get_or_insert_with(|| Box::new(ActiveColorTransitions(HashMap::new())))
                .0
                .insert(
                    property,
                    ActiveTransition {
                        from,
                        to,
                        current: from,
                        duration_ms: transition.duration.get(),
                        delay_ms: transition.delay.get(),
                        easing: transition.easing,
                        start_ms: None,
                    },
                );
            started = true;
        }

        let mut current = target;
        if entry
            .color_transitions
            .as_deref()
            .is_some_and(|transitions| transitions.0.is_empty())
        {
            entry.color_transitions = None;
        }
        if let Some(transitions) = entry.color_transitions.as_deref() {
            for (property, transition) in &transitions.0 {
                set_box_color(&mut current, *property, transition.current.into_paint());
            }
        }
        self.surface.set_box_paint(node, current)?;
        if started {
            crate::runtime::runtime_wake::wake_runtime();
        }
        Ok(())
    }

    fn configure_text_color_transition(
        &mut self,
        element: Element,
        previous_target: RgbaColor,
        previous_current: RgbaColor,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (target, transition) = {
            let entry = self.element(element)?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target =
                RgbaColor::from_paint(&lower_color(resolved.computed().inherited_text().color()));
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(transition.property, ComputedTransitionProperty::All)
                        || transition.property
                            == ComputedTransitionProperty::Property(StyleProperty::Color)
                })
                .copied();
            (target, transition)
        };

        let entry = self.element_mut(element)?;
        if !was_initialized || Some(previous_target) == target {
            return Ok(());
        }
        let Some((target, transition)) =
            target.zip(transition.filter(|transition| transition.duration.get() > 0.0))
        else {
            entry.text_color_transition = None;
            return Ok(());
        };
        entry.text_color_transition = Some(Box::new(ActiveTransition {
            from: previous_current,
            to: target,
            current: previous_current,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
        }));
        crate::runtime::runtime_wake::wake_runtime();
        Ok(())
    }

    fn configure_transform_transition(
        &mut self,
        element: Element,
        previous_target: &ComputedTransformStyle,
        previous_current: &ComputedTransformStyle,
        was_initialized: bool,
    ) -> Result<(), RuntimeBindingError> {
        let (node, target, transition) = {
            let entry = self.element(element)?;
            let node = entry
                .node
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let resolved = entry
                .resolved
                .as_ref()
                .ok_or(RuntimeBindingError::UnknownElement { element })?;
            let target = resolved.computed().paint().transform.clone();
            let transition = resolved
                .computed()
                .motion()
                .transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(transition.property, ComputedTransitionProperty::All)
                        || transition.property
                            == ComputedTransitionProperty::Property(StyleProperty::Transform)
                })
                .copied();
            (node, target, transition)
        };

        let entry = self.element_mut(element)?;
        if !was_initialized {
            entry.transform_transition = None;
            return Ok(());
        }
        if previous_target.functions == target.functions {
            let current = entry.transform_transition.as_deref_mut().map(|active| {
                let from_functions = active.from.functions.clone();
                let to_functions = active.to.functions.clone();
                let current_functions = active.current.functions.clone();
                active.from = target.clone();
                active.from.functions = from_functions;
                active.to = target.clone();
                active.to.functions = to_functions;
                active.current = target;
                active.current.functions = current_functions;
                active.current.clone()
            });
            if let Some(current) = current {
                Self::apply_transform_update(node, &current, &mut self.surface)?;
            }
            return Ok(());
        }
        let Some(transition) = transition.filter(|transition| transition.duration.get() > 0.0)
        else {
            entry.transform_transition = None;
            return Ok(());
        };
        let mut from = target.clone();
        from.functions = previous_current.functions.clone();
        if interpolate_transform_style(&from, &target, 0.0).is_none() {
            entry.transform_transition = None;
            return Ok(());
        }
        entry.transform_transition = Some(Box::new(ActiveTransition {
            from: from.clone(),
            to: target,
            current: from,
            duration_ms: transition.duration.get(),
            delay_ms: transition.delay.get(),
            easing: transition.easing,
            start_ms: None,
        }));
        let current = entry
            .transform_transition
            .as_deref()
            .expect("transition was installed above")
            .current
            .clone();
        Self::apply_transform_update(node, &current, &mut self.surface)?;
        crate::runtime::runtime_wake::wake_runtime();
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

    fn element_subtree(&self, root: Element) -> Result<Vec<Element>, RuntimeBindingError> {
        let mut elements = Vec::new();
        let mut pending = vec![root];
        while let Some(element) = pending.pop() {
            let entry = self.element(element)?;
            if entry.node.is_none() {
                continue;
            }
            elements.push(element);
            pending.extend(entry.children.iter().rev().copied());
        }
        Ok(elements)
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
            let entry = state.element(handle)?;
            if &entry.specified == style {
                if !entry.style_initialized {
                    state.element_mut(handle)?.style_initialized = true;
                }
                return Ok(());
            }
            let previous = entry.specified.clone();
            let previous_target = entry
                .resolved
                .as_ref()
                .map_or(1.0, |style| style.computed().paint().opacity.get());
            let previous_current = entry
                .opacity_transition
                .as_deref()
                .map_or(previous_target, |transition| transition.current);
            let previous_paint = entry
                .resolved
                .as_ref()
                .map(|resolved| {
                    let computed = resolved.computed();
                    lower_paint(computed.paint(), computed.layout()).box_paint
                })
                .unwrap_or_default();
            let previous_current_colors = entry
                .color_transitions
                .as_deref()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter())
                .map(|(property, transition)| (*property, transition.current))
                .collect::<HashMap<_, _>>();
            let previous_transform = entry
                .resolved
                .as_ref()
                .map(|resolved| resolved.computed().paint().transform.clone())
                .unwrap_or_default();
            let previous_layout_targets = entry
                .resolved
                .as_ref()
                .map(layout_animation_values)
                .unwrap_or_default();
            let previous_layout_current = entry
                .layout_transitions
                .as_deref()
                .into_iter()
                .flat_map(|transitions| transitions.0.iter())
                .map(|(property, transition)| (*property, transition.current.clone()))
                .collect::<HashMap<_, _>>();
            let previous_current_transform = entry.transform_transition.as_deref().map_or_else(
                || previous_transform.clone(),
                |transition| transition.current.clone(),
            );
            let was_initialized = entry.style_initialized;
            let text_color_snapshots = state
                .element_subtree(handle)?
                .into_iter()
                .filter_map(|element| {
                    let entry = state.elements.get(&element)?;
                    let resolved = entry.resolved.as_ref()?;
                    let target = RgbaColor::from_paint(&lower_color(
                        resolved.computed().inherited_text().color(),
                    ))?;
                    let current = entry
                        .text_color_transition
                        .as_deref()
                        .map_or(target, |transition| transition.current);
                    Some((element, target, current, entry.style_initialized))
                })
                .collect::<Vec<_>>();
            state.element_mut(handle)?.specified = style.clone();
            if let Err(error) = state.apply_subtree(handle) {
                state.element_mut(handle)?.specified = previous;
                return Err(error);
            }
            state.configure_layout_transitions(
                handle,
                &previous_layout_targets,
                &previous_layout_current,
                was_initialized,
            )?;
            state.configure_keyframe_animations(handle)?;
            state.configure_opacity_transition(
                handle,
                previous_target,
                previous_current,
                was_initialized,
            )?;
            state.configure_color_transitions(
                handle,
                &previous_paint,
                &previous_current_colors,
                was_initialized,
            )?;
            state.configure_transform_transition(
                handle,
                &previous_transform,
                &previous_current_transform,
                was_initialized,
            )?;
            {
                let state = &mut *state;
                BindingState::reapply_active_transitions(&state.elements, &mut state.surface)?;
            }
            for (element, target, current, initialized) in text_color_snapshots {
                state.configure_text_color_transition(element, target, current, initialized)?;
            }
            let text_updates = BindingState::active_text_color_updates(&state.elements);
            BindingState::apply_text_color_updates(text_updates, &mut state.surface)?;
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

#[cfg(test)]
mod motion_tests {
    use super::*;

    fn opacity_animation(
        delay_ms: f32,
        iterations: MotionIterationCount,
        direction: MotionDirection,
        fill_mode: MotionFillMode,
        play_state: MotionPlayState,
    ) -> ActiveKeyframeAnimation {
        let mut animation = ActiveKeyframeAnimation {
            declaration: AnimationValue {
                name: Some("test".to_owned()),
                keyframes: None,
                duration: whisker_engine::whisker_style::MotionTime::milliseconds(100.0),
                easing: MotionEasing::Linear,
                delay: whisker_engine::whisker_style::MotionTime::milliseconds(delay_ms),
                iteration_count: iterations,
                direction,
                fill_mode,
                play_state,
            },
            tracks: vec![KeyframePropertyTrack {
                property: StyleProperty::Opacity,
                points: vec![
                    KeyframePoint {
                        offset: 0.0,
                        value: AnimatedPropertyValue::Number(0.0),
                        easing: None,
                    },
                    KeyframePoint {
                        offset: 1.0,
                        value: AnimatedPropertyValue::Number(1.0),
                        easing: None,
                    },
                ],
            }],
            current_time_ms: 0.0,
            last_timestamp_ms: None,
            current: HashMap::new(),
            finished: false,
        };
        animation.sample_current_time();
        animation
    }

    fn opacity_sample(animation: &ActiveKeyframeAnimation) -> Option<f32> {
        match animation.current.get(&StyleProperty::Opacity) {
            Some(AnimatedPropertyValue::Number(value)) => Some(*value),
            _ => None,
        }
    }

    #[test]
    fn keyframe_timeline_honors_delay_fill_iterations_and_direction() {
        let mut animation = opacity_animation(
            50.0,
            MotionIterationCount::Count(StyleNumber::new(2.0)),
            MotionDirection::Alternate,
            MotionFillMode::Both,
            MotionPlayState::Running,
        );
        assert_eq!(opacity_sample(&animation), Some(0.0));

        animation.sample(1_000.0);
        animation.sample(1_100.0);
        assert_eq!(opacity_sample(&animation), Some(0.5));
        animation.sample(1_200.0);
        assert_eq!(opacity_sample(&animation), Some(0.5));
        animation.sample(1_250.0);
        assert_eq!(opacity_sample(&animation), Some(0.0));
        assert!(animation.finished);
        assert!(!animation.needs_frame());
    }

    #[test]
    fn paused_keyframe_timeline_keeps_its_hold_time_when_resumed() {
        let mut animation = opacity_animation(
            0.0,
            MotionIterationCount::Count(StyleNumber::new(1.0)),
            MotionDirection::Normal,
            MotionFillMode::Forwards,
            MotionPlayState::Running,
        );
        animation.sample(1_000.0);
        animation.sample(1_040.0);
        assert_eq!(opacity_sample(&animation), Some(0.4));

        animation.declaration.play_state = MotionPlayState::Paused;
        animation.sample(5_000.0);
        assert_eq!(opacity_sample(&animation), Some(0.4));
        assert!(!animation.needs_frame());

        animation.declaration.play_state = MotionPlayState::Running;
        animation.last_timestamp_ms = None;
        animation.sample(9_000.0);
        assert_eq!(opacity_sample(&animation), Some(0.4));
        animation.sample(9_010.0);
        assert_eq!(opacity_sample(&animation), Some(0.5));
    }

    #[test]
    fn negative_delay_seeks_into_the_first_iteration() {
        let mut animation = opacity_animation(
            -25.0,
            MotionIterationCount::Count(StyleNumber::new(1.0)),
            MotionDirection::Normal,
            MotionFillMode::None,
            MotionPlayState::Running,
        );
        assert_eq!(opacity_sample(&animation), Some(0.25));
        animation.sample(10.0);
        animation.sample(35.0);
        assert_eq!(opacity_sample(&animation), Some(0.5));
    }

    fn hsla(hue_degrees: f32) -> PaintColor {
        PaintColor::Hsla {
            hue_degrees,
            saturation: 100.0,
            lightness: 50.0,
            alpha: 1.0,
        }
    }

    #[test]
    fn hsl_and_transparent_colors_canonicalize_to_srgb() {
        for (hue, expected) in [
            (0.0, (255, 0, 0)),
            (60.0, (255, 255, 0)),
            (120.0, (0, 255, 0)),
            (180.0, (0, 255, 255)),
            (240.0, (0, 0, 255)),
            (300.0, (255, 0, 255)),
            (360.0, (255, 0, 0)),
        ] {
            let color = RgbaColor::from_paint(&hsla(hue)).unwrap().into_paint();
            assert_eq!(
                color,
                PaintColor::Srgba {
                    red: expected.0,
                    green: expected.1,
                    blue: expected.2,
                    alpha: 1.0,
                }
            );
        }
        assert!(RgbaColor::from_paint(&PaintColor::Named("red".into())).is_none());
        assert_eq!(
            RgbaColor::from_paint(&PaintColor::Named("TRANSPARENT".into()))
                .unwrap()
                .into_paint(),
            PaintColor::Srgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: 0.0,
            }
        );
    }

    #[test]
    fn color_interpolation_uses_premultiplied_alpha() {
        let transparent_red = RgbaColor {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 0.0,
        };
        let opaque_blue = RgbaColor {
            red: 0.0,
            green: 0.0,
            blue: 1.0,
            alpha: 1.0,
        };
        assert_eq!(
            transparent_red.interpolate(opaque_blue, 0.5).into_paint(),
            PaintColor::Srgba {
                red: 0,
                green: 0,
                blue: 255,
                alpha: 0.5,
            }
        );
        assert_eq!(
            transparent_red
                .interpolate(transparent_red, 0.5)
                .into_paint(),
            PaintColor::Srgba {
                red: 0,
                green: 0,
                blue: 0,
                alpha: 0.0,
            }
        );
    }

    #[test]
    fn compatible_transform_functions_interpolate_with_identity_padding() {
        let number = StyleNumber::new;
        let length = |value| ComputedLengthPercentage::new(value, value / 100.0);
        let cases = [
            (
                ComputedTransformFunction::Translate {
                    x: length(0.0),
                    y: length(10.0),
                    z: number(20.0),
                },
                ComputedTransformFunction::Translate {
                    x: length(100.0),
                    y: length(30.0),
                    z: number(40.0),
                },
            ),
            (
                ComputedTransformFunction::RotateX(number(0.0)),
                ComputedTransformFunction::RotateX(number(90.0)),
            ),
            (
                ComputedTransformFunction::RotateY(number(0.0)),
                ComputedTransformFunction::RotateY(number(90.0)),
            ),
            (
                ComputedTransformFunction::RotateZ(number(0.0)),
                ComputedTransformFunction::RotateZ(number(90.0)),
            ),
            (
                ComputedTransformFunction::Scale {
                    x: number(1.0),
                    y: number(2.0),
                    z: number(3.0),
                },
                ComputedTransformFunction::Scale {
                    x: number(3.0),
                    y: number(4.0),
                    z: number(5.0),
                },
            ),
            (
                ComputedTransformFunction::Skew {
                    x_degrees: number(0.0),
                    y_degrees: number(10.0),
                },
                ComputedTransformFunction::Skew {
                    x_degrees: number(20.0),
                    y_degrees: number(30.0),
                },
            ),
        ];
        for (from, to) in cases {
            assert!(interpolate_transform_function(&from, &to, 0.5).is_some());
            let from = ComputedTransformStyle {
                functions: vec![from],
                ..ComputedTransformStyle::default()
            };
            let to = ComputedTransformStyle {
                functions: vec![to],
                origin_x: ComputedLengthPercentage::new(12.0, 0.0),
                ..ComputedTransformStyle::default()
            };
            let current = interpolate_transform_style(&from, &to, 0.5).unwrap();
            assert_eq!(current.origin_x, to.origin_x);
            assert_eq!(current.functions.len(), 1);
            assert!(
                interpolate_transform_style(&ComputedTransformStyle::default(), &to, 0.5).is_some()
            );
            assert!(
                interpolate_transform_style(&to, &ComputedTransformStyle::default(), 0.5).is_some()
            );
        }
    }

    #[test]
    fn incompatible_and_matrix_transform_functions_require_decomposition() {
        let number = StyleNumber::new;
        let matrix = ComputedTransformFunction::Matrix([number(1.0); 16]);
        assert!(identity_transform_function(&matrix).is_none());
        assert!(
            interpolate_transform_function(
                &matrix,
                &ComputedTransformFunction::Matrix([number(2.0); 16]),
                0.5,
            )
            .is_none()
        );
        assert!(
            interpolate_transform_function(
                &ComputedTransformFunction::RotateX(number(0.0)),
                &ComputedTransformFunction::RotateY(number(90.0)),
                0.5,
            )
            .is_none()
        );
        assert!(
            interpolate_transform_style(
                &ComputedTransformStyle::default(),
                &ComputedTransformStyle::default(),
                0.5,
            )
            .is_some()
        );
    }
}
