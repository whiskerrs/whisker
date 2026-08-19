//! Host-driven lifecycle and frame execution for one application surface.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::runtime::RuntimeContext;
use crate::runtime::host_wake::RuntimeWakeHandle;
use crate::runtime::reactive::{self, Owner};
use crate::runtime::view::{self, Element};
use crate::{InputDispatch, RuntimeFrame, RuntimeFrameError, RuntimeInputError, SurfaceRuntime};
use whisker_engine::whisker_layout::LayoutSize;
use whisker_engine::whisker_protocol::{ApplyResult, InputEvent, MeasurementReady};
use whisker_engine::{DeferredMeasurementApply, FrameSink, HostLayoutOptions, MeasurementHost};

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
pub enum RuntimeDriveError<HostError, SinkError> {
    /// Frame delivery is invalid for the current lifecycle.
    Lifecycle(RuntimeLifecycleError),
    /// Measurement, layout, or presentation failed.
    Frame(RuntimeFrameError<HostError, SinkError>),
    /// A queued Host event failed validation or routing.
    Input(RuntimeInputError),
}

impl<HostError: fmt::Debug, SinkError: fmt::Debug> fmt::Display
    for RuntimeDriveError<HostError, SinkError>
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Whisker runtime drive error: {self:?}")
    }
}

impl<HostError, SinkError> Error for RuntimeDriveError<HostError, SinkError>
where
    HostError: Error + 'static,
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
    wake_enabled: Arc<AtomicBool>,
}

impl RuntimeInstance {
    /// Creates an unmounted runtime connected to one Host wake-up endpoint.
    pub fn new(surface: SurfaceRuntime, host_wake: RuntimeWakeHandle) -> Self {
        let wake_enabled = Arc::new(AtomicBool::new(false));
        let gate = Arc::clone(&wake_enabled);
        let forwarded = host_wake.clone();
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

    /// Processes ready tasks and reactive changes, then produces one frame.
    #[allow(clippy::too_many_arguments)]
    pub fn drive_frame<Host: MeasurementHost, Sink: FrameSink>(
        &self,
        timestamp_ms: f64,
        viewport: LayoutSize,
        environment_epoch: u64,
        viewport_epoch: u32,
        measurement_host: &mut Host,
        frame_sink: &mut Sink,
        options: HostLayoutOptions,
    ) -> Result<RuntimeDrive, RuntimeDriveError<Host::Error, Sink::Error>> {
        self.require(RuntimeLifecycle::Running, "drive a frame")
            .map_err(RuntimeDriveError::Lifecycle)?;
        let surface = self.surface.clone();
        self.context.enter(|| {
            view::with_installed_renderer(surface.renderer(), || {
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
                        viewport,
                        environment_epoch,
                        viewport_epoch,
                        measurement_host,
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
        let dispatch = self.surface.dispatch_input(event)?;
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
        crate::runtime::host_wake::wake_runtime();
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
