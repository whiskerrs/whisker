//! Runtime ownership of one retained semantic surface.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use crate::ElementRegistry;
use crate::ElementRegistryError;
use crate::background_resources::{
    BackgroundProjection, BackgroundResourceError, BackgroundResourceManager,
};
use crate::element::ElementTag;
use crate::event::Dataset;
use crate::transform_interpolation::interpolate_transform_style;
#[cfg(test)]
use crate::transform_interpolation::{identity_transform_function, interpolate_transform_function};
use crate::value::WhiskerValue;
use crate::view::{BindType, DynRenderer, Element, LayoutObservation, with_installed_renderer};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    Accessibility, BoxPaint, ElementRegistration, ElementSchema, ElementValueKind, HitTestBehavior,
    HostPresentationUpdate, InputEvent, InputEventError, MeasurementReady, NodeId, PaintColor,
    ResourceCommand, ResourceEvent, ResourceId, ResourceMessageError, SurfaceId,
};
#[cfg(test)]
use whisker_engine::whisker_style::ComputedTransformFunction;
use whisker_engine::whisker_style::{
    AnimationValue, ComputedFlexBasis, ComputedLayoutStyle, ComputedLengthPercentage,
    ComputedLengthPercentageAuto, ComputedSizeValue, ComputedTransformStyle,
    ComputedTransitionProperty, InheritedStyle, MotionDirection, MotionEasing, MotionFillMode,
    MotionIterationCount, MotionPlayState, ResolvedNodeStyle, SpecifiedStyle, StyleEnvironment,
    StyleNumber, StyleProperty, StyleResolutionError, resolve_style,
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
    /// Coalesced Host presentation state contained invalid numeric data.
    InvalidPresentation,
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
    /// A re-entrant Host callback was accepted for ordered delivery at the
    /// next safe runtime boundary.
    Queued,
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

/// First-class runtime for one retained surface populated by the authoring renderer.
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
        Self::with_element_registry_and_protocol(
            surface,
            environment,
            registry,
            whisker_protocol::ProtocolVersion::CURRENT,
        )
    }

    /// Creates a registered surface using the protocol selected with its Host.
    pub fn with_element_registry_and_protocol(
        surface: SurfaceId,
        environment: StyleEnvironment,
        registry: ElementRegistry,
        protocol: whisker_protocol::ProtocolVersion,
    ) -> Self {
        Self {
            state: Rc::new(RefCell::new(BindingState {
                surface: SurfaceEngine::with_protocol(surface, protocol),
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
                mutation_batch: None,
                #[cfg(test)]
                surface_snapshot_count: 0,
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

    /// Returns the first rejected runtime mutation that has not yet been
    /// reported at a runtime boundary.
    pub fn binding_error(&self) -> Option<RuntimeBindingError> {
        self.state.borrow().error.clone()
    }

    #[cfg(test)]
    pub(crate) fn reset_surface_snapshot_count(&self) {
        self.state.borrow_mut().surface_snapshot_count = 0;
    }

    #[cfg(test)]
    pub(crate) fn surface_snapshot_count(&self) -> usize {
        self.state.borrow().surface_snapshot_count
    }

    /// Returns the environment used for viewport-relative style resolution.
    pub fn environment(&self) -> StyleEnvironment {
        self.state.borrow().environment
    }

    pub(crate) fn begin_mutation_batch(&self) {
        self.state.borrow_mut().begin_mutation_batch();
    }

    pub(crate) fn finish_mutation_batch(&self) -> Result<(), RuntimeBindingError> {
        self.state.borrow_mut().finish_mutation_batch()
    }

    pub(crate) fn defer_binding_error(&self, error: RuntimeBindingError) {
        self.state.borrow_mut().defer_binding_error(error);
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

    pub(crate) fn has_input_listener(&self, target: NodeId, event: &str) -> bool {
        let state = self.state.borrow();
        state.root.is_some_and(|root| {
            state
                .plan_event(root, target, event)
                .is_ok_and(|firings| !firings.is_empty())
        })
    }

    /// Hit-tests and routes one Host-normalized event through Rust listeners.
    pub fn dispatch_input(&self, event: &InputEvent) -> Result<InputDispatch, RuntimeInputError> {
        self.dispatch_input_with_presentation(event, &[])
    }

    /// Applies coalesced Host presentation state, then hit-tests and routes one event.
    pub fn dispatch_input_with_presentation(
        &self,
        event: &InputEvent,
        presentation: &[HostPresentationUpdate],
    ) -> Result<InputDispatch, RuntimeInputError> {
        let (target, firings, body) = {
            let mut state = self.state.borrow_mut();
            state
                .take_binding_error()
                .map_err(RuntimeInputError::Binding)?;
            if event.surface != state.surface.surface() {
                return Err(RuntimeInputError::SurfaceMismatch {
                    expected: state.surface.surface(),
                    received: event.surface,
                });
            }
            event.validate().map_err(RuntimeInputError::InvalidInput)?;
            for update in presentation {
                if !update.is_valid() {
                    return Err(RuntimeInputError::InvalidPresentation);
                }
            }
            for update in presentation {
                match *update {
                    HostPresentationUpdate::ScrollOffset { node, offset } => {
                        // The Host can observe a scroll callback immediately
                        // before a committed frame removes that native node.
                        // Keep newer updates in the same coalesced batch and
                        // route the input against the current retained scene.
                        if state.surface.node(node).is_none() {
                            continue;
                        }
                        state
                            .surface
                            .update_host_scroll_offset(node, [offset.x, offset.y])
                            .map_err(RuntimeBindingError::from)
                            .map_err(RuntimeInputError::Binding)?;
                    }
                }
            }
            let root = state.root.ok_or(RuntimeInputError::MissingRoot)?;
            let captured = event
                .pointer
                .and_then(|pointer| state.surface.pointer_capture_target(pointer.id));
            let target = if let Some(captured) = captured {
                Some(captured)
            } else if let Some(target) = event.target {
                if state.surface.node(target).is_none() {
                    // A Host may have queued this event while the preceding
                    // frame removed its target. Treat that normal race as an
                    // unhandled event rather than poisoning the surface.
                    return Ok(InputDispatch::default());
                }
                Some(target)
            } else if let Some(pointer) = event.pointer {
                state
                    .surface
                    .hit_test(root, pointer.position)
                    .map_err(RuntimeBindingError::from)
                    .map_err(RuntimeInputError::Binding)?
            } else {
                None
            };
            let Some(target) = target else {
                return Ok(InputDispatch::default());
            };
            let event_name = event.kind.name(event.pointer.map(|pointer| pointer.kind));
            let firings = state
                .plan_event(root, target, event_name)?
                .into_iter()
                .map(|(current_target, callback)| (callback, state.target_value(current_target)))
                .collect::<Vec<_>>();
            (
                target,
                firings,
                input_body(event, state.target_value(target)),
            )
        };

        let listener_count = firings.len();
        for (callback, current_target) in firings {
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
        state.take_binding_error()?;
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
        crate::runtime_wake::wake_runtime();
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
        let (layout, notifications, batch_end_notifications) = {
            let mut state = self.state.borrow_mut();
            state
                .take_binding_error()
                .map_err(RuntimeLayoutError::Binding)?;
            state
                .flush_background_projections()
                .map_err(RuntimeLayoutError::Binding)?;
            let root = state.root.ok_or(RuntimeLayoutError::MissingRoot)?;
            let layout = state
                .surface
                .drive_layout(root, viewport, environment_epoch, provider, options)
                .map_err(RuntimeLayoutError::Measurement)?;
            let transform_updates =
                BindingState::active_transform_updates(&state.elements, &state.surface);
            BindingState::apply_transform_updates(transform_updates, &mut state.surface)
                .map_err(RuntimeLayoutError::Binding)?;
            let notifications = if layout.has_layout() {
                let mut notifications = Vec::new();
                let BindingState {
                    surface, elements, ..
                } = &mut *state;
                if let Some(last_layout) = surface.last_layout() {
                    for entry in elements.values_mut() {
                        let Some(observers) = entry.layout_observers.as_mut() else {
                            continue;
                        };
                        let Some((geometry, participation)) = entry.node.and_then(|node| {
                            last_layout
                                .get_with_participation(node)
                                .map(|(geometry, participation)| (*geometry, participation))
                        }) else {
                            continue;
                        };
                        let observation = LayoutObservation {
                            geometry,
                            margin_box_size: entry
                                .node
                                .and_then(|node| last_layout.margin_box_size(node))
                                .expect("observed layout node"),
                            participation,
                        };
                        if observers.last_notified == Some(observation) {
                            continue;
                        }
                        observers.last_notified = Some(observation);
                        notifications.extend(
                            observers
                                .callbacks
                                .iter()
                                .cloned()
                                .map(|callback| (callback, observation)),
                        );
                    }
                }
                notifications
            } else {
                Vec::new()
            };
            let batch_end_notifications = if layout.has_layout() {
                state
                    .elements
                    .values()
                    .flat_map(|entry| entry.layout_batch_end_observers.iter().cloned())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            (layout, notifications, batch_end_notifications)
        };
        if !notifications.is_empty() || !batch_end_notifications.is_empty() {
            with_installed_renderer(self.renderer(), || {
                for (callback, observation) in notifications {
                    callback(observation);
                }
                for callback in batch_end_notifications {
                    callback();
                }
            });
        }
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
        state
            .take_binding_error()
            .map_err(RuntimePresentError::Binding)?;
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
    id: String,
    dataset: BTreeMap<String, WhiskerValue>,
    accessibility: Accessibility,
    listeners: HashMap<String, Vec<RuntimeListener>>,
    layout_observers: Option<Box<LayoutObservers>>,
    layout_batch_end_observers: Vec<Rc<dyn Fn() + 'static>>,
    style_initialized: bool,
    opacity_transition: Option<Box<ActiveTransition<f32>>>,
    color_transitions: Option<Box<ActiveColorTransitions>>,
    text_color_transition: Option<Box<ActiveTransition<RgbaColor>>>,
    transform_transition: Option<Box<ActiveTransition<ComputedTransformStyle>>>,
    layout_transitions: Option<Box<ActivePropertyTransitions>>,
    animations: Vec<ActiveKeyframeAnimation>,
    pending_motion_events: VecDeque<PendingMotionEvent>,
}

#[derive(Clone, Default)]
struct LayoutObservers {
    callbacks: Vec<Rc<dyn Fn(LayoutObservation) + 'static>>,
    last_notified: Option<LayoutObservation>,
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

    fn receives_text_style(&self) -> bool {
        matches!(
            self,
            Self::Registered { registration } if registration.text_style
        )
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
    mutation_batch: Option<MutationBatch>,
    #[cfg(test)]
    surface_snapshot_count: usize,
}

struct MutationBatch {
    // Batches are nestable so future runtime boundaries can compose without
    // accidentally committing an outer event halfway through.
    depth: usize,
    // Preserve first-dirtied order for deterministic frame construction.
    dirty_elements: Vec<Element>,
    // The first snapshot wins when one handler writes the same style more
    // than once: transitions run from the pre-event value to the final value.
    style_changes: Vec<(Element, PendingStyleChange)>,
}

struct PendingStyleChange {
    previous: SpecifiedStyle,
    snapshots: Vec<MotionSnapshot>,
}

struct EnvironmentStyleUpdate {
    element: Element,
    node: NodeId,
    resolved: ResolvedNodeStyle,
    text: Option<PlainTextInput>,
}

impl BindingState {
    fn target_value(&self, node: NodeId) -> WhiskerValue {
        let metadata = self
            .node_elements
            .get(&node)
            .and_then(|element| self.elements.get(element));
        WhiskerValue::map([
            (
                "id",
                WhiskerValue::String(metadata.map(|entry| entry.id.clone()).unwrap_or_default()),
            ),
            ("uid", WhiskerValue::Int(node.get() as i64)),
            (
                "dataset",
                WhiskerValue::Map(
                    metadata
                        .map(|entry| entry.dataset.clone())
                        .unwrap_or_default(),
                ),
            ),
        ])
    }

    fn begin_mutation_batch(&mut self) {
        if let Some(batch) = &mut self.mutation_batch {
            batch.depth += 1;
            return;
        }
        self.mutation_batch = Some(MutationBatch {
            depth: 1,
            dirty_elements: Vec::new(),
            style_changes: Vec::new(),
        });
    }

    fn finish_mutation_batch(&mut self) -> Result<(), RuntimeBindingError> {
        let Some(batch) = &mut self.mutation_batch else {
            return Ok(());
        };
        if batch.depth > 1 {
            batch.depth -= 1;
            return Ok(());
        }
        let batch = self
            .mutation_batch
            .take()
            .expect("the outermost mutation batch remains installed");
        let recorded_error = self.error.take();
        let roots = self.minimal_dirty_roots(batch.dirty_elements);
        if let Err(error) = self.apply_subtrees_now(&roots) {
            Self::restore_style_changes(&mut self.elements, batch.style_changes);
            return Err(error);
        }
        let snapshots = batch
            .style_changes
            .into_iter()
            .flat_map(|(_, change)| change.snapshots)
            // An event may style an element and dispose its owning view in
            // the same reactive flush (an instant route pop is the common
            // case). The retained-scene mutation has already removed that
            // element, so its pre-event motion snapshot is stale and must not
            // be configured against the now-released handle. A parent style
            // change can capture snapshots for a whole subtree, hence this
            // filters individual snapshots rather than only top-level style
            // changes.
            .filter(|snapshot| self.elements.contains_key(&snapshot.element))
            .collect::<Vec<_>>();
        if !snapshots.is_empty()
            && let Err(error) = self.configure_style_motion(snapshots)
        {
            return Err(error);
        }
        recorded_error.map_or(Ok(()), Err)
    }

    fn restore_style_changes(
        elements: &mut HashMap<Element, BoundElement>,
        changes: Vec<(Element, PendingStyleChange)>,
    ) {
        for (element, change) in changes {
            if let Some(entry) = elements.get_mut(&element) {
                entry.specified = change.previous;
            }
        }
    }

    fn mark_subtree_dirty(&mut self, element: Element) {
        let Some(batch) = &mut self.mutation_batch else {
            return;
        };
        if !batch.dirty_elements.contains(&element) {
            batch.dirty_elements.push(element);
        }
    }

    fn minimal_dirty_roots(&self, dirty: Vec<Element>) -> Vec<Element> {
        let dirty = dirty
            .into_iter()
            .filter(|element| self.elements.contains_key(element))
            .collect::<Vec<_>>();
        let dirty_set = dirty.iter().copied().collect::<HashSet<_>>();
        dirty
            .into_iter()
            .filter(|element| {
                let mut parent = self.elements.get(element).and_then(|entry| entry.parent);
                while let Some(candidate) = parent {
                    if dirty_set.contains(&candidate) {
                        return false;
                    }
                    parent = self.elements.get(&candidate).and_then(|entry| entry.parent);
                }
                true
            })
            .collect()
    }

    fn take_binding_error(&mut self) -> Result<(), RuntimeBindingError> {
        match self.error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn record(&mut self, result: Result<(), RuntimeBindingError>) {
        match result {
            Ok(()) => crate::runtime_wake::wake_runtime(),
            Err(error) if self.error.is_none() => self.error = Some(error),
            Err(_) => {}
        }
    }

    fn defer_binding_error(&mut self, error: RuntimeBindingError) {
        if self.error.is_none() {
            self.error = Some(error);
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
        self.take_binding_error()?;
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
            if self.element(update.element)?.kind.receives_text_style() {
                surface.set_text_style(update.node, update.resolved.computed())?;
            }
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
                id: String::new(),
                dataset: BTreeMap::new(),
                accessibility: Accessibility::default(),
                listeners: HashMap::new(),
                layout_observers: None,
                layout_batch_end_observers: Vec::new(),
                style_initialized: false,
                opacity_transition: None,
                color_transitions: None,
                text_color_transition: None,
                transform_transition: None,
                layout_transitions: None,
                animations: Vec::new(),
                pending_motion_events: VecDeque::new(),
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
        if self.mutation_batch.is_some() {
            self.element(element)?;
            self.mark_subtree_dirty(element);
            return Ok(());
        }
        self.apply_subtrees_now(&[element])
    }

    fn apply_subtrees_now(&mut self, elements: &[Element]) -> Result<(), RuntimeBindingError> {
        if elements.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        {
            self.surface_snapshot_count += 1;
        }
        // One speculative snapshot preserves the existing all-or-nothing
        // lowering contract for every dirty root in this event.
        let mut surface = self.surface.clone();
        let mut background_resources = self.background_resources.clone();
        let externally_used = self.externally_used_resource_ids();
        let mut updates = Vec::new();
        let mut resource_commands = Vec::new();
        for element in elements {
            let parent_style = self
                .element(*element)?
                .parent
                .and_then(|parent| self.elements.get(&parent))
                .and_then(|parent| parent.resolved.as_ref())
                .map(|resolved| resolved.inherited_for_children().clone());
            self.prepare_subtree(
                *element,
                parent_style.as_ref(),
                &mut surface,
                &mut background_resources,
                &externally_used,
                &mut updates,
                &mut resource_commands,
            )?;
        }
        Self::reapply_active_transitions(&self.elements, &mut surface)?;

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
        if entry.kind.receives_text_style() {
            surface.set_text_style(node, resolved.computed())?;
        }
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

    fn motion_snapshots(&self, root: Element) -> Result<Vec<MotionSnapshot>, RuntimeBindingError> {
        self.element_subtree(root)?
            .into_iter()
            .map(|element| {
                let entry = self.element(element)?;
                let resolved = entry
                    .resolved
                    .as_ref()
                    .ok_or(RuntimeBindingError::UnknownElement { element })?;
                let computed = resolved.computed();
                let opacity_target = computed.paint().opacity.get();
                let opacity_current = entry
                    .opacity_transition
                    .as_deref()
                    .map_or(opacity_target, |transition| transition.current);
                let box_paint = lower_paint(computed.paint(), computed.layout()).box_paint;
                let current_colors = entry
                    .color_transitions
                    .as_deref()
                    .into_iter()
                    .flat_map(|transitions| transitions.0.iter())
                    .map(|(property, transition)| (*property, transition.current))
                    .collect();
                let transform_target = computed.paint().transform.clone();
                let transform_current = entry.transform_transition.as_deref().map_or_else(
                    || transform_target.clone(),
                    |transition| {
                        entry
                            .node
                            .and_then(|node| self.surface.node(node))
                            .and_then(|node| node.layout())
                            .and_then(|layout| {
                                interpolate_transform_style(
                                    &transition.from,
                                    &transition.to,
                                    transition.current_progress,
                                    layout.border_box.width,
                                    layout.border_box.height,
                                )
                            })
                            .unwrap_or_else(|| transition.current.clone())
                    },
                );
                let layout_targets = layout_animation_values(resolved);
                let layout_current = entry
                    .layout_transitions
                    .as_deref()
                    .into_iter()
                    .flat_map(|transitions| transitions.0.iter())
                    .map(|(property, transition)| (*property, transition.current.clone()))
                    .collect();
                let text_color_target =
                    RgbaColor::from_paint(&lower_color(computed.inherited_text().color()));
                let text_color_current = text_color_target.map(|target| {
                    entry
                        .text_color_transition
                        .as_deref()
                        .map_or(target, |transition| transition.current)
                });
                Ok(MotionSnapshot {
                    element,
                    resolved: resolved.clone(),
                    initialized: entry.style_initialized,
                    layout_targets,
                    layout_current,
                    opacity_target,
                    opacity_current,
                    box_paint,
                    current_colors,
                    transform_target,
                    transform_current,
                    text_color_target,
                    text_color_current,
                })
            })
            .collect()
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
    ) -> Result<(), RuntimeBindingError> {
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
        self.surface.invoke_command(node, command, arguments)?;
        Ok(())
    }
}

const EVENT_POINTER: u64 = 1 << 0;
const EVENT_ACTIVATION: u64 = 1 << 1;
const EVENT_NAMED: u64 = 1 << 2;

#[cfg(test)]
mod motion_tests;

#[cfg(test)]
mod input_tests {
    use super::*;
    use crate::element::ElementTag;
    use crate::view::create_element;
    use whisker_protocol::{InputEventKind, InputPoint, SurfaceId, WhiskerValue};

    #[test]
    fn stale_scroll_updates_do_not_discard_current_updates_or_input() {
        crate::reactive::__reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(91).unwrap(),
            StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
        );
        let mut runtime =
            crate::RuntimeInstance::new(surface.clone(), crate::RuntimeWakeHandle::new(|| {}));
        runtime.mount(|| create_element(ElementTag::View)).unwrap();
        let root = surface.root().unwrap();
        let stale = NodeId::new(u64::MAX).unwrap();

        let dispatch = runtime
            .dispatch_input_with_presentation(
                &InputEvent {
                    surface: surface.surface(),
                    timestamp_ms: 1.0,
                    kind: InputEventKind::Click,
                    pointer: None,
                    target: Some(root),
                    detail: WhiskerValue::Null,
                },
                &[
                    HostPresentationUpdate::ScrollOffset {
                        node: stale,
                        offset: InputPoint { x: 1.0, y: 2.0 },
                    },
                    HostPresentationUpdate::ScrollOffset {
                        node: root,
                        offset: InputPoint { x: 3.0, y: 4.0 },
                    },
                ],
            )
            .unwrap();

        assert!(!dispatch.queued);
        assert_eq!(
            surface
                .state
                .borrow()
                .surface
                .node(root)
                .unwrap()
                .host_scroll_offset(),
            [3.0, 4.0]
        );
    }
}

#[cfg(test)]
mod layout_observer_tests {
    use std::cell::{Cell, RefCell};
    use std::convert::Infallible;
    use std::rc::Rc;

    use super::*;
    use crate::element::ElementTag;
    use crate::view::{
        append_child, create_element, observe_layout, observe_layout_batch_end,
        set_specified_style, with_installed_renderer,
    };
    use whisker_engine::whisker_layout::LayoutParticipation;
    use whisker_engine::whisker_protocol::{MeasurementRequest, MeasurementResponse};
    use whisker_engine::whisker_style::{
        DisplayValue, LengthPercentageValue, LengthUnit, LengthValue, PositionValue, SizeValue,
        StyleNumber, StyleProperty, StyleValue,
    };

    struct NoMeasurements;

    impl MeasurementProvider for NoMeasurements {
        type Error = Infallible;

        fn measure_batch(
            &mut self,
            _surface: SurfaceId,
            requests: &[MeasurementRequest],
            _responses: &mut Vec<MeasurementResponse>,
        ) -> Result<(), Self::Error> {
            assert!(requests.is_empty());
            Ok(())
        }
    }

    fn absolute_box(width: f32, height: f32) -> SpecifiedStyle {
        let px = |value| {
            StyleValue::Size(SizeValue::LengthPercentage(LengthPercentageValue::Length(
                LengthValue::Dimension {
                    value: StyleNumber::new(value),
                    unit: LengthUnit::Px,
                },
            )))
        };
        SpecifiedStyle::new()
            .push(
                StyleProperty::Position,
                StyleValue::Position(PositionValue::Absolute),
            )
            .push(StyleProperty::Width, px(width))
            .push(StyleProperty::Height, px(height))
    }

    #[test]
    fn layout_observers_skip_unchanged_geometry_after_another_node_relayouts() {
        crate::reactive::__reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(92).unwrap(),
            StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
        );
        let mut runtime =
            crate::RuntimeInstance::new(surface.clone(), crate::RuntimeWakeHandle::new(|| {}));
        let observed = Rc::new(Cell::new(0));
        let completed_batches = Rc::new(Cell::new(0));
        let sibling = Rc::new(Cell::new(None));

        runtime
            .mount({
                let observed = Rc::clone(&observed);
                let completed_batches = Rc::clone(&completed_batches);
                let sibling = Rc::clone(&sibling);
                move || {
                    let root = create_element(ElementTag::View);
                    let child = create_element(ElementTag::View);
                    let other = create_element(ElementTag::View);
                    set_specified_style(child, &absolute_box(20.0, 20.0));
                    set_specified_style(other, &absolute_box(10.0, 10.0));
                    append_child(root, child);
                    append_child(root, other);
                    observe_layout(child, Box::new(move |_| observed.set(observed.get() + 1)));
                    observe_layout_batch_end(
                        root,
                        Box::new(move || completed_batches.set(completed_batches.get() + 1)),
                    );
                    sibling.set(Some(other));
                    root
                }
            })
            .unwrap();

        let mut provider = NoMeasurements;
        surface
            .drive_layout(
                LayoutSize::new(320.0, 480.0),
                1,
                &mut provider,
                LayoutOptions::default(),
            )
            .unwrap();
        assert_eq!(observed.get(), 1);
        assert_eq!(completed_batches.get(), 1);

        with_installed_renderer(surface.renderer(), || {
            set_specified_style(sibling.get().unwrap(), &absolute_box(30.0, 10.0));
        });
        surface
            .drive_layout(
                LayoutSize::new(320.0, 480.0),
                1,
                &mut provider,
                LayoutOptions::default(),
            )
            .unwrap();

        assert_eq!(
            observed.get(),
            1,
            "an unrelated layout pass must not re-notify unchanged geometry"
        );
        assert_eq!(
            completed_batches.get(),
            2,
            "batch-end observers run once after each completed layout notification batch"
        );
    }

    #[test]
    fn layout_observers_report_display_none_separately_from_zero_geometry() {
        crate::reactive::__reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(93).unwrap(),
            StyleEnvironment::new(320.0, 480.0, 1.0, 14.0),
        );
        let mut runtime =
            crate::RuntimeInstance::new(surface.clone(), crate::RuntimeWakeHandle::new(|| {}));
        let observations = Rc::new(RefCell::new(Vec::new()));
        let root_handle = Rc::new(Cell::new(None));

        runtime
            .mount({
                let observations = Rc::clone(&observations);
                let root_handle = Rc::clone(&root_handle);
                move || {
                    let root = create_element(ElementTag::View);
                    let child = create_element(ElementTag::View);
                    set_specified_style(root, &absolute_box(100.0, 100.0));
                    set_specified_style(child, &absolute_box(0.0, 0.0));
                    append_child(root, child);
                    observe_layout(
                        child,
                        Box::new(move |observation| observations.borrow_mut().push(observation)),
                    );
                    root_handle.set(Some(root));
                    root
                }
            })
            .unwrap();

        let mut provider = NoMeasurements;
        surface
            .drive_layout(
                LayoutSize::new(320.0, 480.0),
                1,
                &mut provider,
                LayoutOptions::default(),
            )
            .unwrap();
        assert_eq!(observations.borrow().len(), 1);
        assert_eq!(
            observations.borrow()[0].participation,
            LayoutParticipation::Participating
        );
        assert_eq!(observations.borrow()[0].geometry.border_box.width, 0.0);
        assert_eq!(observations.borrow()[0].geometry.border_box.height, 0.0);

        with_installed_renderer(surface.renderer(), || {
            let hidden = absolute_box(100.0, 100.0).push(
                StyleProperty::Display,
                StyleValue::Display(DisplayValue::None),
            );
            set_specified_style(root_handle.get().unwrap(), &hidden);
        });
        surface
            .drive_layout(
                LayoutSize::new(320.0, 480.0),
                1,
                &mut provider,
                LayoutOptions::default(),
            )
            .unwrap();
        assert_eq!(observations.borrow().len(), 2);
        assert_eq!(
            observations.borrow()[1].participation,
            LayoutParticipation::SuppressedByDisplayNone
        );

        with_installed_renderer(surface.renderer(), || {
            set_specified_style(root_handle.get().unwrap(), &absolute_box(100.0, 100.0));
        });
        surface
            .drive_layout(
                LayoutSize::new(320.0, 480.0),
                1,
                &mut provider,
                LayoutOptions::default(),
            )
            .unwrap();
        assert_eq!(observations.borrow().len(), 3);
        assert_eq!(
            observations.borrow()[2].participation,
            LayoutParticipation::Participating
        );
    }
}

mod event;
mod renderer;

use event::*;

mod motion;

use motion::*;
