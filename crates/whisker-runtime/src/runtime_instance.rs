//! Host-driven lifecycle and frame execution for one application surface.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::RuntimeContext;
use crate::module::{ModuleHost, with_module_host};
use crate::reactive::{self, Owner};
use crate::runtime_wake::RuntimeWakeHandle;
use crate::view::{self, Element};
use crate::{
    InputDispatch, ResourceEventApply, RuntimeFrame, RuntimeFrameError, RuntimeInputError,
    RuntimeResourceError, SurfaceRuntime,
};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{
    ApplyResult, InputEvent, InputEventKind, InputPoint, MeasurementReady, NodeId, PointerId,
    PointerKind, ResourceEvent, WhiskerValue,
};
use whisker_engine::whisker_style::StyleEnvironment;
use whisker_engine::{DeferredMeasurementApply, FrameSink, LayoutOptions, MeasurementProvider};

/// Lifecycle of one Host-mounted runtime instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeLifecycle {
    /// Allocated but application code has not run.
    Created,
    /// Mounted and eligible to receive input and frames.
    Running,
    /// Retained but not eligible for frame delivery.
    Paused,
    /// Disposed permanently.
    Unmounted,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::element::ElementTag;
    use crate::module::{ModuleHost, ModuleSubscription, PlatformModule, with_module_host};
    use crate::reactive::{__reset_for_tests, RwSignal, effect};
    use crate::view::{
        BindType, append_child, create_element, remove_child, set_attribute, set_event_listener,
        set_specified_style,
    };
    use whisker_engine::whisker_protocol::{InputEventKind, SurfaceId};
    use whisker_style::{SpecifiedStyle, StyleEnvironment, StyleNumber, StyleProperty, StyleValue};

    #[test]
    fn one_input_event_takes_one_surface_snapshot_for_text_and_style_updates() {
        __reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(77).unwrap(),
            StyleEnvironment::new(320.0, 100.0, 1.0, 14.0),
        );
        let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
        runtime
            .mount(|| {
                let root = create_element(ElementTag::View);
                let first = create_element(ElementTag::Text);
                let first_text = create_element(ElementTag::RawText);
                let second = create_element(ElementTag::Text);
                let second_text = create_element(ElementTag::RawText);
                append_child(first, first_text);
                append_child(second, second_text);
                append_child(root, first);
                append_child(root, second);
                set_event_listener(
                    root,
                    "scroll",
                    BindType::Bind,
                    Box::new(move |_| {
                        set_attribute(first_text, "text", "first-updated");
                        set_attribute(second_text, "text", "second-updated");
                        let style = SpecifiedStyle::new().push(
                            StyleProperty::Opacity,
                            StyleValue::Number(StyleNumber::new(0.5)),
                        );
                        set_specified_style(first, &style);
                        set_specified_style(second, &style);
                    }),
                );
                root
            })
            .unwrap();

        surface.reset_surface_snapshot_count();
        runtime
            .dispatch_input(&InputEvent {
                surface: surface.surface(),
                timestamp_ms: 1.0,
                kind: InputEventKind::Named("scroll".to_owned()),
                pointer: None,
                target: surface.root(),
                detail: WhiskerValue::Null,
            })
            .unwrap();

        assert_eq!(surface.surface_snapshot_count(), 1);
    }

    #[test]
    fn one_input_event_can_style_and_release_an_element_in_the_same_batch() {
        __reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(77).unwrap(),
            StyleEnvironment::new(320.0, 100.0, 1.0, 14.0),
        );
        let mut runtime = RuntimeInstance::new(surface.clone(), RuntimeWakeHandle::new(|| {}));
        runtime
            .mount(|| {
                let root = create_element(ElementTag::View);
                let transient_owner = Owner::new(None);
                let transient = transient_owner.with(|| create_element(ElementTag::View));
                append_child(root, transient);
                set_event_listener(
                    root,
                    "dismiss",
                    BindType::Bind,
                    Box::new(move |_| {
                        let style = SpecifiedStyle::new().push(
                            StyleProperty::Opacity,
                            StyleValue::Number(StyleNumber::new(0.0)),
                        );
                        set_specified_style(transient, &style);
                        remove_child(root, transient);
                        transient_owner.dispose();
                    }),
                );
                root
            })
            .unwrap();

        runtime
            .dispatch_input(&InputEvent {
                surface: surface.surface(),
                timestamp_ms: 1.0,
                kind: InputEventKind::Named("dismiss".to_owned()),
                pointer: None,
                target: surface.root(),
                detail: WhiskerValue::Null,
            })
            .expect("releasing a just-styled element must not poison the mutation batch");
    }

    #[test]
    fn module_events_run_inside_the_owning_runtime_context() {
        __reset_for_tests();
        let first_surface = SurfaceRuntime::new(
            SurfaceId::new(77).unwrap(),
            StyleEnvironment::new(320.0, 100.0, 1.0, 14.0),
        );
        let second_surface = SurfaceRuntime::new(
            SurfaceId::new(78).unwrap(),
            StyleEnvironment::new(320.0, 100.0, 1.0, 14.0),
        );
        let mut first = RuntimeInstance::new(first_surface, RuntimeWakeHandle::new(|| {}));
        let mut second = RuntimeInstance::new(second_surface, RuntimeWakeHandle::new(|| {}));
        let first_host = ModuleHost::new(|_, _, _, _, _| false, |_, _, _| {});
        let second_host = ModuleHost::new(|_, _, _, _, _| false, |_, _, _| {});
        let first_observed = Rc::new(Cell::new(0));
        let second_observed = Rc::new(Cell::new(0));
        let first_subscription = Rc::new(RefCell::new(None::<ModuleSubscription>));
        let second_subscription = Rc::new(RefCell::new(None::<ModuleSubscription>));

        with_module_host(&first_host, || {
            first
                .mount({
                    let observed = Rc::clone(&first_observed);
                    let subscription = Rc::clone(&first_subscription);
                    move || {
                        let signal = RwSignal::new(0_i64);
                        effect(move || observed.set(signal.get()));
                        *subscription.borrow_mut() = Some(PlatformModule::named("demo").on_event(
                            "tick",
                            move |payload| match payload {
                                WhiskerValue::Int(value) => signal.set(value),
                                _ => panic!("expected integer module payload"),
                            },
                        ));
                        create_element(ElementTag::View)
                    }
                })
                .unwrap();
        });
        with_module_host(&second_host, || {
            second
                .mount({
                    let observed = Rc::clone(&second_observed);
                    let subscription = Rc::clone(&second_subscription);
                    move || {
                        let signal = RwSignal::new(0_i64);
                        effect(move || observed.set(signal.get()));
                        *subscription.borrow_mut() = Some(PlatformModule::named("demo").on_event(
                            "tick",
                            move |payload| match payload {
                                WhiskerValue::Int(value) => signal.set(value),
                                _ => panic!("expected integer module payload"),
                            },
                        ));
                        create_element(ElementTag::View)
                    }
                })
                .unwrap();
        });

        assert!(
            first
                .dispatch_module_event(&first_host, "demo", "tick", WhiskerValue::Int(11))
                .unwrap()
        );
        assert_eq!(first_observed.get(), 11);
        assert_eq!(second_observed.get(), 0);

        assert!(
            second
                .dispatch_module_event(&second_host, "demo", "tick", WhiskerValue::Int(22))
                .unwrap()
        );
        assert_eq!(first_observed.get(), 11);
        assert_eq!(second_observed.get(), 22);

        first
            .context
            .enter(|| first_subscription.borrow_mut().take());
        second
            .context
            .enter(|| second_subscription.borrow_mut().take());
    }

    #[test]
    fn root_remount_replaces_only_the_instance_application_tree() {
        __reset_for_tests();
        let surface = SurfaceRuntime::new(
            SurfaceId::new(79).unwrap(),
            StyleEnvironment::new(320.0, 100.0, 1.0, 14.0),
        );
        let wake_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wake_observer = Arc::clone(&wake_count);
        let mut runtime = RuntimeInstance::new(
            surface.clone(),
            RuntimeWakeHandle::new(move || {
                wake_observer.fetch_add(1, Ordering::Relaxed);
            }),
        );
        let first = runtime.mount(|| create_element(ElementTag::View)).unwrap();
        let second = runtime
            .remount_root(|| create_element(ElementTag::Text))
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Running);
        assert!(wake_count.load(Ordering::Relaxed) >= 2);
        runtime.unmount().unwrap();
    }
}

/// A lifecycle operation that is invalid in the current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLifecycleError {
    /// State observed by the rejected operation.
    pub state: RuntimeLifecycle,
    /// Operation that was rejected.
    pub operation: &'static str,
}

impl fmt::Display for RuntimeLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot {} a Whisker runtime in {:?} state",
            self.operation, self.state
        )
    }
}

impl Error for RuntimeLifecycleError {}

/// Failure while delivering one Host event to a runtime instance.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeEventError {
    /// Event delivery is invalid for the current lifecycle.
    Lifecycle(RuntimeLifecycleError),
    /// Validation, hit testing, or listener lookup failed.
    Input(RuntimeInputError),
    /// Resource completion validation or generation matching failed.
    Resource(RuntimeResourceError),
}

impl fmt::Display for RuntimeEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime event error: {self:?}")
    }
}

impl Error for RuntimeEventError {}

/// Outcome of one Host frame callback.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeDrive {
    /// Measurement, layout, and presentation performed during the callback.
    pub frame: RuntimeFrame,
    /// Whether the Host should schedule another VSync after this callback.
    pub needs_frame: bool,
}

/// Failure while validating lifecycle or producing a frame.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeDriveError<MeasurementError, SinkError> {
    /// Frame delivery is invalid for the current lifecycle.
    Lifecycle(RuntimeLifecycleError),
    /// Measurement, layout, or presentation failed.
    Frame(RuntimeFrameError<MeasurementError, SinkError>),
    /// A queued Host event failed validation or routing.
    Input(RuntimeInputError),
    /// Host viewport values could not be applied to the retained style environment.
    Environment(crate::RuntimeBindingError),
    /// Rust-owned transition sampling could not update the retained scene.
    Motion(crate::RuntimeBindingError),
}

impl<MeasurementError: fmt::Debug, SinkError: fmt::Debug> fmt::Display
    for RuntimeDriveError<MeasurementError, SinkError>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime drive error: {self:?}")
    }
}

impl<MeasurementError, SinkError> Error for RuntimeDriveError<MeasurementError, SinkError>
where
    MeasurementError: Error + 'static,
    SinkError: Error + 'static,
{
}

/// One isolated, Host-driven Whisker application runtime.
///
/// The instance owns no thread or event loop. The Host retains this value and
/// calls [`Self::drive_frame`] from its UI frame callback. Reactive state,
/// local futures, view bookkeeping, and animation state are isolated by its
/// [`RuntimeContext`] even when several instances share the same UI thread.
pub struct RuntimeInstance {
    context: RuntimeContext,
    surface: SurfaceRuntime,
    wake: RuntimeWakeHandle,
    owner: Option<Owner>,
    lifecycle: RuntimeLifecycle,
    pending_input: RefCell<VecDeque<InputEvent>>,
    activations: RefCell<ActivationRecognizer>,
    wake_enabled: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug)]
struct ActivationCandidate {
    target: NodeId,
    origin: InputPoint,
    started_at_ms: f64,
    pointer_kind: PointerKind,
    cancelled: bool,
}

struct RecognizedActivation {
    tap: InputEvent,
    emits_click: bool,
}

fn merge_input_dispatch(dispatch: &mut InputDispatch, synthesized: InputDispatch) {
    dispatch.target = synthesized.target.or(dispatch.target);
    dispatch.consumed |= synthesized.consumed;
    dispatch.listener_count += synthesized.listener_count;
    dispatch.queued |= synthesized.queued;
}

#[derive(Default)]
struct ActivationRecognizer {
    pointers: HashMap<PointerId, ActivationCandidate>,
}

impl ActivationRecognizer {
    fn observe(
        &mut self,
        event: &InputEvent,
        hit_target: Option<NodeId>,
    ) -> Option<RecognizedActivation> {
        let pointer = event.pointer?;
        match event.kind {
            InputEventKind::PointerDown => {
                if let Some(target) = hit_target {
                    self.pointers.insert(
                        pointer.id,
                        ActivationCandidate {
                            target,
                            origin: pointer.position,
                            started_at_ms: event.timestamp_ms,
                            pointer_kind: pointer.kind,
                            cancelled: false,
                        },
                    );
                }
                None
            }
            InputEventKind::PointerMove => {
                let candidate = self.pointers.get_mut(&pointer.id)?;
                let dx = pointer.position.x - candidate.origin.x;
                let dy = pointer.position.y - candidate.origin.y;
                if dx * dx + dy * dy > TAP_SLOP * TAP_SLOP {
                    candidate.cancelled = true;
                }
                None
            }
            InputEventKind::PointerCancel => {
                self.pointers.remove(&pointer.id);
                None
            }
            InputEventKind::PointerUp => {
                let candidate = self.pointers.remove(&pointer.id)?;
                let elapsed = event.timestamp_ms - candidate.started_at_ms;
                if candidate.cancelled
                    || !(0.0..=TAP_TIMEOUT_MS).contains(&elapsed)
                    || hit_target != Some(candidate.target)
                {
                    return None;
                }
                Some(RecognizedActivation {
                    tap: InputEvent {
                        surface: event.surface,
                        timestamp_ms: event.timestamp_ms,
                        kind: InputEventKind::Tap,
                        pointer: Some(pointer),
                        target: Some(candidate.target),
                        detail: WhiskerValue::Null,
                    },
                    emits_click: candidate.pointer_kind == PointerKind::Mouse,
                })
            }
            _ => None,
        }
    }

    fn clear(&mut self) {
        self.pointers.clear();
    }
}

const TAP_SLOP: f32 = 10.0;
const TAP_TIMEOUT_MS: f64 = 500.0;

impl RuntimeInstance {
    /// Creates an unmounted runtime connected to one Host wake-up endpoint.
    pub fn new(surface: SurfaceRuntime, wake: RuntimeWakeHandle) -> Self {
        let wake_enabled = Arc::new(AtomicBool::new(false));
        let gate = Arc::clone(&wake_enabled);
        let forwarded = wake.clone();
        let wake = RuntimeWakeHandle::new(move || {
            if gate.load(Ordering::Acquire) {
                forwarded.wake();
            }
        });
        Self {
            context: RuntimeContext::new(wake.clone()),
            surface,
            wake,
            owner: None,
            lifecycle: RuntimeLifecycle::Created,
            pending_input: RefCell::new(VecDeque::new()),
            activations: RefCell::new(ActivationRecognizer::default()),
            wake_enabled,
        }
    }

    /// Returns the current lifecycle state.
    pub const fn lifecycle(&self) -> RuntimeLifecycle {
        self.lifecycle
    }

    /// Returns the retained surface driven by this instance.
    pub const fn surface(&self) -> &SurfaceRuntime {
        &self.surface
    }

    /// Runs application construction once and requests the initial frame.
    pub fn mount(
        &mut self,
        application: impl FnOnce() -> Element,
    ) -> Result<Element, RuntimeLifecycleError> {
        self.require(RuntimeLifecycle::Created, "mount")?;
        let surface = self.surface.clone();
        let (owner, root) = self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                crate::drain_runtime_dispatches();
                let owner = Owner::new(None);
                let root = owner.with(application);
                view::set_root(root);
                reactive::flush();
                reactive::flush_mounts();
                (owner, root)
            })
        });
        self.owner = Some(owner);
        self.lifecycle = RuntimeLifecycle::Running;
        self.wake_enabled.store(true, Ordering::Release);
        self.wake.wake();
        Ok(root)
    }

    /// Suspends reactive effects and frame delivery while retaining state.
    pub fn pause(&mut self) -> Result<(), RuntimeLifecycleError> {
        self.require(RuntimeLifecycle::Running, "pause")?;
        self.wake_enabled.store(false, Ordering::Release);
        self.activations.borrow_mut().clear();
        let owner = self.owner.expect("a running runtime has a root owner");
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || owner.pause());
        });
        self.lifecycle = RuntimeLifecycle::Paused;
        Ok(())
    }

    /// Resumes a paused instance and requests a frame for deferred changes.
    pub fn resume(&mut self) -> Result<(), RuntimeLifecycleError> {
        self.require(RuntimeLifecycle::Paused, "resume")?;
        self.wake_enabled.store(true, Ordering::Release);
        let owner = self.owner.expect("a paused runtime has a root owner");
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                crate::drain_runtime_dispatches();
                owner.resume();
            });
        });
        self.lifecycle = RuntimeLifecycle::Running;
        self.wake.wake();
        Ok(())
    }

    /// Permanently disposes application ownership and retained nodes.
    pub fn unmount(&mut self) -> Result<(), RuntimeLifecycleError> {
        if !matches!(
            self.lifecycle,
            RuntimeLifecycle::Running | RuntimeLifecycle::Paused
        ) {
            return Err(RuntimeLifecycleError {
                state: self.lifecycle,
                operation: "unmount",
            });
        }
        let owner = self.owner.take().expect("a mounted runtime has an owner");
        self.wake_enabled.store(false, Ordering::Release);
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || owner.dispose());
        });
        self.pending_input.borrow_mut().clear();
        self.activations.borrow_mut().clear();
        self.context.shutdown();
        self.lifecycle = RuntimeLifecycle::Unmounted;
        Ok(())
    }

    /// Rebuilds the application root inside this instance without replacing
    /// the Host-owned surface or wake endpoint.
    ///
    /// Development adapters use this as the conservative fallback when a
    /// code update cannot be reflected through an individual component. The
    /// runtime deliberately knows nothing about patch transports or dynamic
    /// libraries; it only provides the instance-scoped remount transaction.
    pub fn remount_root(
        &mut self,
        application: impl FnOnce() -> Element,
    ) -> Result<Element, RuntimeEventError> {
        self.require(RuntimeLifecycle::Running, "remount the application root")
            .map_err(RuntimeEventError::Lifecycle)?;
        let previous = self
            .owner
            .take()
            .expect("a running runtime has a root owner");
        let surface = self.surface.clone();
        let result = self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                surface.begin_mutation_batch();
                previous.dispose();
                let owner = Owner::new(None);
                let root = owner.with(application);
                view::set_root(root);
                reactive::flush();
                reactive::flush_mounts();
                surface
                    .finish_mutation_batch()
                    .map_err(RuntimeInputError::Binding)
                    .map_err(RuntimeEventError::Input)?;
                Ok((owner, root))
            })
        });
        match result {
            Ok((owner, root)) => {
                self.owner = Some(owner);
                self.wake.wake();
                Ok(root)
            }
            Err(error) => {
                // The previous owner has already been disposed. Keeping the
                // lifecycle running would let later Host callbacks enter a
                // half-mounted instance, so make the failure terminal.
                self.wake_enabled.store(false, Ordering::Release);
                self.lifecycle = RuntimeLifecycle::Unmounted;
                self.context.shutdown();
                Err(error)
            }
        }
    }

    /// Rebuilds mounted component sites whose body function appears in
    /// `patched_functions`.
    ///
    /// Function pointers are an opaque matching key supplied by a
    /// development adapter. This API performs no code loading and remains
    /// useful to any future patch engine that can identify changed bodies.
    pub fn remount_components(
        &self,
        patched_functions: &[*const ()],
    ) -> Result<reactive::RemountStats, RuntimeEventError> {
        self.require(RuntimeLifecycle::Running, "remount updated components")
            .map_err(RuntimeEventError::Lifecycle)?;
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                surface.begin_mutation_batch();
                let stats = reactive::remount_components_for(patched_functions);
                reactive::flush();
                reactive::flush_mounts();
                surface
                    .finish_mutation_batch()
                    .map_err(RuntimeInputError::Binding)
                    .map_err(RuntimeEventError::Input)?;
                Ok(stats)
            })
        })
    }

    /// Delivers one Host-normalized event and flushes its reactive transaction.
    pub fn dispatch_input(&self, event: &InputEvent) -> Result<InputDispatch, RuntimeEventError> {
        self.require(RuntimeLifecycle::Running, "dispatch input")
            .map_err(RuntimeEventError::Lifecycle)?;
        if self.context.is_entered() {
            if event.surface != self.surface.surface() {
                return Err(RuntimeEventError::Input(
                    RuntimeInputError::SurfaceMismatch {
                        expected: self.surface.surface(),
                        received: event.surface,
                    },
                ));
            }
            event
                .validate()
                .map_err(RuntimeInputError::InvalidInput)
                .map_err(RuntimeEventError::Input)?;
            const INPUT_CAP: usize = 4096;
            let mut pending = self.pending_input.borrow_mut();
            if pending.len() >= INPUT_CAP {
                return Err(RuntimeEventError::Input(
                    RuntimeInputError::InputQueueFull { limit: INPUT_CAP },
                ));
            }
            pending.push_back(event.clone());
            return Ok(InputDispatch {
                queued: true,
                ..InputDispatch::default()
            });
        }
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                crate::drain_runtime_dispatches();
                let dispatch = self
                    .dispatch_input_active(event)
                    .map_err(RuntimeEventError::Input)?;
                self.drain_pending_input()
                    .map_err(RuntimeEventError::Input)?;
                Ok(dispatch)
            })
        })
    }

    /// Delivers one native-module event inside this instance's renderer and
    /// reactive transaction.
    ///
    /// Module objects may be process-wide on the Host, but subscriptions and
    /// application callbacks belong to exactly one runtime instance. Paused
    /// instances may retain state changes; their owner resumes effects later.
    pub fn dispatch_module_event(
        &self,
        modules: &Rc<ModuleHost>,
        module: &str,
        event: &str,
        payload: WhiskerValue,
    ) -> Result<bool, RuntimeEventError> {
        if !matches!(
            self.lifecycle,
            RuntimeLifecycle::Running | RuntimeLifecycle::Paused
        ) {
            return Err(RuntimeEventError::Lifecycle(RuntimeLifecycleError {
                state: self.lifecycle,
                operation: "dispatch a module event",
            }));
        }
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                with_module_host(modules, || {
                    if self.lifecycle == RuntimeLifecycle::Running {
                        crate::drain_runtime_dispatches();
                    }
                    surface.begin_mutation_batch();
                    let dispatched = modules.dispatch_event(module, event, payload);
                    reactive::flush();
                    reactive::flush_mounts();
                    surface
                        .finish_mutation_batch()
                        .map_err(RuntimeInputError::Binding)
                        .map_err(RuntimeEventError::Input)?;
                    Ok(dispatched)
                })
            })
        })
    }

    /// Applies one deferred intrinsic measurement result.
    ///
    /// A running instance requests another frame when the result invalidates
    /// layout. A paused instance retains the result until [`Self::resume`].
    pub fn measurement_ready(
        &self,
        ready: &MeasurementReady,
    ) -> Result<DeferredMeasurementApply, RuntimeEventError> {
        if !matches!(
            self.lifecycle,
            RuntimeLifecycle::Running | RuntimeLifecycle::Paused
        ) {
            return Err(RuntimeEventError::Lifecycle(RuntimeLifecycleError {
                state: self.lifecycle,
                operation: "apply deferred measurement",
            }));
        }
        let surface = self.surface.clone();
        let apply = self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                if self.lifecycle == RuntimeLifecycle::Running {
                    crate::drain_runtime_dispatches();
                }
                surface
                    .apply_measurement_ready(ready)
                    .map_err(RuntimeInputError::Binding)
                    .map_err(RuntimeEventError::Input)
            })
        })?;
        if self.lifecycle == RuntimeLifecycle::Running
            && matches!(apply, DeferredMeasurementApply::Applied { .. })
        {
            self.wake.wake();
        }
        Ok(apply)
    }

    /// Applies one typed Host resource completion.
    ///
    /// Replaced and released generations are accepted as stale no-ops. A
    /// current completion requests a frame while the runtime is running; a
    /// paused runtime retains it until [`Self::resume`].
    pub fn dispatch_resource_event(
        &self,
        event: &ResourceEvent,
    ) -> Result<ResourceEventApply, RuntimeEventError> {
        if !matches!(
            self.lifecycle,
            RuntimeLifecycle::Running | RuntimeLifecycle::Paused
        ) {
            return Err(RuntimeEventError::Lifecycle(RuntimeLifecycleError {
                state: self.lifecycle,
                operation: "apply resource completion",
            }));
        }
        let apply = self
            .surface
            .apply_resource_event(event)
            .map_err(RuntimeEventError::Resource)?;
        if self.lifecycle == RuntimeLifecycle::Running && apply == ResourceEventApply::Applied {
            self.wake.wake();
        }
        Ok(apply)
    }

    /// Applies current Host viewport metrics, processes ready tasks and reactive
    /// changes, then produces one frame.
    ///
    /// The logical layout viewport is derived from `environment`, preventing
    /// viewport-relative style resolution and Taffy root constraints from using
    /// different dimensions.
    #[allow(clippy::too_many_arguments)]
    pub fn drive_frame<Provider: MeasurementProvider, Sink: FrameSink>(
        &self,
        timestamp_ms: f64,
        environment: StyleEnvironment,
        environment_epoch: u64,
        viewport_epoch: u32,
        measurement_provider: &mut Provider,
        frame_sink: &mut Sink,
        options: LayoutOptions,
    ) -> Result<RuntimeDrive, RuntimeDriveError<Provider::Error, Sink::Error>> {
        self.require(RuntimeLifecycle::Running, "drive a frame")
            .map_err(RuntimeDriveError::Lifecycle)?;
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
                surface
                    .update_environment(environment)
                    .map_err(RuntimeDriveError::Environment)?;
                crate::drain_runtime_dispatches();
                self.drain_pending_input()
                    .map_err(RuntimeDriveError::Input)?;
                crate::anim_hook::step(timestamp_ms);
                reactive::flush();
                crate::tasks::run_until_stalled();
                reactive::flush();
                reactive::flush_mounts();
                surface
                    .step_motion(timestamp_ms)
                    .map_err(RuntimeDriveError::Motion)?;

                let frame = surface
                    .render_frame(
                        LayoutSize::new(
                            environment.viewport_width(),
                            environment.viewport_height(),
                        ),
                        environment_epoch,
                        viewport_epoch,
                        measurement_provider,
                        frame_sink,
                        options,
                    )
                    .map_err(RuntimeDriveError::Frame)?;
                let recovery = matches!(frame.presentation, Some(ApplyResult::NeedSnapshot { .. }));
                self.drain_pending_input()
                    .map_err(RuntimeDriveError::Input)?;
                Ok(RuntimeDrive {
                    frame,
                    needs_frame: recovery
                        || reactive::has_pending_work()
                        || surface.has_active_motion()
                        || !self.pending_input.borrow().is_empty(),
                })
            })
        })
    }

    fn dispatch_input_active(
        &self,
        event: &InputEvent,
    ) -> Result<InputDispatch, RuntimeInputError> {
        // Treat listener execution and the reactive cascade it schedules as
        // one retained-scene transaction. Renderer calls still update the
        // element mirror immediately, while expensive style/subtree lowering
        // is committed once after the queue settles.
        self.surface.begin_mutation_batch();
        let dispatch = (|| {
            let mut dispatch = self.surface.dispatch_input(event)?;
            let activation = self
                .activations
                .borrow_mut()
                .observe(event, dispatch.target);
            if let Some(activation) = activation {
                let synthesized = self.surface.dispatch_input(&activation.tap)?;
                merge_input_dispatch(&mut dispatch, synthesized);
                if activation.emits_click {
                    let click = InputEvent {
                        kind: InputEventKind::Click,
                        ..activation.tap
                    };
                    let synthesized = self.surface.dispatch_input(&click)?;
                    merge_input_dispatch(&mut dispatch, synthesized);
                }
            }
            reactive::flush();
            reactive::flush_mounts();
            Ok(dispatch)
        })();
        let finish = self
            .surface
            .finish_mutation_batch()
            .map_err(RuntimeInputError::Binding);
        match dispatch {
            Ok(dispatch) => {
                finish?;
                Ok(dispatch)
            }
            Err(error) => {
                let _ = finish;
                Err(error)
            }
        }
    }

    fn drain_pending_input(&self) -> Result<(), RuntimeInputError> {
        const INPUT_CAP: usize = 4096;
        for _ in 0..INPUT_CAP {
            let Some(event) = self.pending_input.borrow_mut().pop_front() else {
                return Ok(());
            };
            self.dispatch_input_active(&event)?;
        }
        crate::runtime_wake::wake_runtime();
        Ok(())
    }

    fn require(
        &self,
        expected: RuntimeLifecycle,
        operation: &'static str,
    ) -> Result<(), RuntimeLifecycleError> {
        if self.lifecycle == expected {
            Ok(())
        } else {
            Err(RuntimeLifecycleError {
                state: self.lifecycle,
                operation,
            })
        }
    }
}
