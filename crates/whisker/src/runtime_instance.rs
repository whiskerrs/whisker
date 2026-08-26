//! Host-driven lifecycle and frame execution for one application surface.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::runtime::RuntimeContext;
use crate::runtime::reactive::{self, Owner};
use crate::runtime::runtime_wake::RuntimeWakeHandle;
use crate::runtime::view::{self, Element};
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

#[derive(Default)]
struct ActivationRecognizer {
    pointers: HashMap<PointerId, ActivationCandidate>,
}

impl ActivationRecognizer {
    fn observe(&mut self, event: &InputEvent, hit_target: Option<NodeId>) -> Option<InputEvent> {
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
                Some(InputEvent {
                    surface: event.surface,
                    timestamp_ms: event.timestamp_ms,
                    kind: if candidate.pointer_kind == PointerKind::Mouse {
                        InputEventKind::Click
                    } else {
                        InputEventKind::Tap
                    },
                    pointer: Some(pointer),
                    target: Some(candidate.target),
                    detail: WhiskerValue::Null,
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
                crate::runtime::drain_runtime_dispatches();
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
                crate::runtime::drain_runtime_dispatches();
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
                crate::runtime::drain_runtime_dispatches();
                let dispatch = self
                    .dispatch_input_active(event)
                    .map_err(RuntimeEventError::Input)?;
                self.drain_pending_input()
                    .map_err(RuntimeEventError::Input)?;
                Ok(dispatch)
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
                    crate::runtime::drain_runtime_dispatches();
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
                crate::runtime::drain_runtime_dispatches();
                self.drain_pending_input()
                    .map_err(RuntimeDriveError::Input)?;
                crate::runtime::anim_hook::step(timestamp_ms);
                reactive::flush();
                crate::runtime::tasks::run_until_stalled();
                reactive::flush();
                reactive::flush_mounts();

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
                        || !self.pending_input.borrow().is_empty(),
                })
            })
        })
    }

    fn dispatch_input_active(
        &self,
        event: &InputEvent,
    ) -> Result<InputDispatch, RuntimeInputError> {
        let mut dispatch = self.surface.dispatch_input(event)?;
        let activation = self
            .activations
            .borrow_mut()
            .observe(event, dispatch.target);
        if let Some(activation) = activation {
            let synthesized = self.surface.dispatch_input(&activation)?;
            dispatch.target = synthesized.target.or(dispatch.target);
            dispatch.consumed |= synthesized.consumed;
            dispatch.listener_count += synthesized.listener_count;
            dispatch.queued |= synthesized.queued;
        }
        reactive::flush();
        reactive::flush_mounts();
        Ok(dispatch)
    }

    fn drain_pending_input(&self) -> Result<(), RuntimeInputError> {
        const INPUT_CAP: usize = 4096;
        for _ in 0..INPUT_CAP {
            let Some(event) = self.pending_input.borrow_mut().pop_front() else {
                return Ok(());
            };
            self.dispatch_input_active(&event)?;
        }
        crate::runtime::runtime_wake::wake_runtime();
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
