//! Shared native Desktop Host services for Whisker.
//!
//! This crate owns only the work shared by macOS, Windows, and Linux:
//! intrinsic measurement, retained frame projection, `wgpu` resources,
//! semantic paint lowering, and one short runtime frame drive. Native window
//! creation, event loops, scale/viewport observation, and redraw scheduling
//! remain in the individual OS crates under `platforms/<os>`.

#![cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
#![warn(missing_docs)]

use std::error::Error;
use std::fmt;

use whisker::RuntimeInstance;
use whisker::runtime::RuntimeWakeHandle;
use whisker_engine::LayoutOptions;
use whisker_protocol::{ElementRegistration, InputEvent, InputEventKind, ResourceId, SurfaceId};
use whisker_style::StyleEnvironment;

/// Marks and defines a platform implementation contributed by a module.
pub use whisker::WhiskerModule;
/// Shared value used by Desktop module properties, functions, and events.
pub use whisker_value::WhiskerValue;

mod element;
mod gpu;
mod input;
mod paint;
mod resource;
mod scene;
mod surface;
mod text;

#[cfg(all(test, feature = "host-conformance"))]
#[path = "../tests/host_conformance/mod.rs"]
mod host_conformance_tests;

use element::DesktopElementRegistry;
pub use element::{
    BuiltInElementModule, DesktopElementFactory, DesktopModuleDefinition, DesktopNativeElement,
    DesktopNativeEvent, DesktopRaster, DesktopRasterError, DesktopViewDefinition,
    DesktopViewImplementation,
};
pub use input::{
    DesktopMouseButton, DesktopPointerAdapter, DesktopPointerEvent, DesktopPointerPhase,
};
/// Desktop Host module declaration, named consistently with native Hosts.
pub type ModuleDefinition = DesktopModuleDefinition;
use gpu::RasterResource;
use resource::{DesktopResourceService, DesktopResourceUpdate};
use surface::DesktopSurface;
use text::NativeTextHost;

/// Logical and physical metrics sampled by an OS adapter for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopFrameContext {
    /// Monotonic Host timestamp in milliseconds.
    pub timestamp_ms: f64,
    /// Current logical viewport width.
    pub logical_width: f32,
    /// Current logical viewport height.
    pub logical_height: f32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// Epoch incremented when metric-affecting Host state changes.
    pub environment_epoch: u64,
    /// Epoch incremented when viewport geometry changes.
    pub viewport_epoch: u32,
}

/// Result returned to the OS adapter after one runtime and paint pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopFrameResult {
    /// Whether the runtime needs another frame without an external wake-up.
    pub needs_frame: bool,
}

/// Failure while initializing or driving shared Desktop Host services.
#[derive(Debug)]
pub struct DesktopError(String);

impl fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for DesktopError {}

/// Host-owned state shared by all native Desktop OS adapters.
///
/// The OS crate owns the window and [`RuntimeInstance`]. This type owns the
/// corresponding measurement provider, retained Host projection, and GPU
/// surface. Calls remain direct Rust calls; no FFI or serialization is added.
pub struct DesktopRuntime {
    measurements: NativeTextHost,
    surface: DesktopSurface,
    resources: DesktopResourceService,
    resource_events: Vec<whisker_protocol::ResourceEvent>,
}

impl DesktopRuntime {
    /// Creates shared Host state for an owned native window target.
    ///
    /// `target` is normally an `Arc<winit::window::Window>` supplied by an OS
    /// adapter. Accepting `wgpu`'s target abstraction keeps `winit` lifecycle
    /// types out of this common crate.
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        physical_size: [u32; 2],
        surface: SurfaceId,
        elements: &[ElementRegistration],
        element_factories: &[DesktopElementFactory],
        resource_wake: RuntimeWakeHandle,
    ) -> Result<Self, DesktopError> {
        let elements = DesktopElementRegistry::bind(elements, element_factories)
            .map_err(|error| DesktopError(format!("bind Desktop elements: {error}")))?;
        let surface = DesktopSurface::new(target, physical_size, surface, elements.clone())
            .await
            .map_err(|error| DesktopError(format!("initialize Desktop renderer: {error}")))?;
        Ok(Self {
            measurements: NativeTextHost::new(elements),
            surface,
            resources: DesktopResourceService::new(std::path::PathBuf::new(), move || {
                resource_wake.wake();
            }),
            resource_events: Vec::new(),
        })
    }

    /// Reconfigures the GPU surface after an OS resize notification.
    pub fn resize(&mut self, physical_size: [u32; 2]) {
        self.surface.resize(physical_size);
    }

    /// Resolves the cursor of the visually topmost hit-testable node at a
    /// logical window position.
    pub fn cursor_at(&self, logical_position: [f32; 2]) -> Option<whisker_protocol::CursorKeyword> {
        self.surface.cursor_at(logical_position)
    }

    /// Resolves the Host-visible target after applying transient scroll state.
    pub fn target_input(&self, event: &mut InputEvent) {
        if event.target.is_none()
            && let Some(pointer) = event.pointer
        {
            event.target = self
                .surface
                .hit_test([pointer.position.x, pointer.position.y]);
        }
    }

    /// Applies a logical wheel/trackpad delta to the nearest scroll container.
    pub fn scroll_at(&mut self, logical_position: [f32; 2], delta: [f32; 2]) -> bool {
        self.surface.scroll_at(logical_position, delta)
    }

    /// Registers one already-decoded RGBA8 raster for later `ResourceId`
    /// background-image references. Acquisition and decoding remain Host work
    /// and do not run in the per-frame protocol path.
    pub fn register_raster_resource(
        &mut self,
        resource: ResourceId,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<(), DesktopError> {
        let raster = RasterResource::new(width, height, pixels)
            .map_err(|error| DesktopError(format!("register Desktop raster: {error}")))?;
        self.surface.register_raster_resource(resource, &raster);
        Ok(())
    }

    /// Starts or releases one Host-owned resource acquisition. Completion is
    /// applied on a later UI-thread frame and exposed through
    /// [`Self::take_resource_events`].
    pub fn resource_command(
        &mut self,
        command: whisker_protocol::ResourceCommand,
    ) -> Result<(), DesktopError> {
        let updates = self
            .resources
            .command(command)
            .map_err(|error| DesktopError(error.to_string()))?;
        self.apply_resource_updates(updates);
        Ok(())
    }

    /// Drains ready/failed resource events for forwarding to the Rust runtime.
    pub fn take_resource_events(&mut self) -> Vec<whisker_protocol::ResourceEvent> {
        self.apply_pending_resource_updates();
        std::mem::take(&mut self.resource_events)
    }

    /// Runs measurement, layout, frame presentation, and native painting once.
    pub fn drive_frame(
        &mut self,
        runtime: &RuntimeInstance,
        context: DesktopFrameContext,
    ) -> Result<DesktopFrameResult, DesktopError> {
        validate_context(context)?;
        self.drain_runtime_resource_commands(runtime)?;
        let dispatched_resource_event = self.dispatch_pending_resource_events(runtime)?;
        let environment = StyleEnvironment::new(
            context.logical_width,
            context.logical_height,
            context.scale,
            14.0,
        );
        let drive = runtime.drive_frame(
            context.timestamp_ms,
            environment,
            context.environment_epoch,
            context.viewport_epoch,
            &mut self.measurements,
            &mut self.surface,
            LayoutOptions::default(),
        );
        self.drain_runtime_resource_commands(runtime)?;
        let drive = drive.map_err(|error| DesktopError(format!("drive Desktop frame: {error}")))?;
        let events = self.surface.take_events();
        let dispatched_provider_event = !events.is_empty();
        for event in events {
            runtime
                .dispatch_input(&InputEvent {
                    surface: runtime.surface().surface(),
                    timestamp_ms: context.timestamp_ms,
                    kind: InputEventKind::Named(event.name),
                    pointer: None,
                    target: Some(event.target),
                    detail: event.detail,
                })
                .map_err(|error| {
                    DesktopError(format!("dispatch Desktop provider event: {error}"))
                })?;
        }
        self.surface
            .paint(
                &mut self.measurements,
                [context.logical_width, context.logical_height],
                context.scale,
            )
            .map_err(|error| DesktopError(format!("paint Desktop frame: {error}")))?;
        Ok(DesktopFrameResult {
            needs_frame: drive.needs_frame
                || dispatched_provider_event
                || dispatched_resource_event,
        })
    }

    fn drain_runtime_resource_commands(
        &mut self,
        runtime: &RuntimeInstance,
    ) -> Result<(), DesktopError> {
        drain_resource_commands(runtime, |command| self.resource_command(command)).map(|_| ())
    }

    fn dispatch_pending_resource_events(
        &mut self,
        runtime: &RuntimeInstance,
    ) -> Result<bool, DesktopError> {
        self.apply_pending_resource_updates();
        dispatch_resource_events(runtime, std::mem::take(&mut self.resource_events))
    }

    fn apply_pending_resource_updates(&mut self) {
        let updates = self.resources.drain();
        self.apply_resource_updates(updates);
    }

    fn apply_resource_updates(&mut self, updates: Vec<DesktopResourceUpdate>) {
        for update in updates {
            match update {
                DesktopResourceUpdate::Ready { event, raster } => {
                    let whisker_protocol::ResourceEvent::Ready { resource, .. } = event else {
                        unreachable!()
                    };
                    self.surface.register_raster_resource(resource, &raster);
                    self.resource_events.push(event);
                }
                DesktopResourceUpdate::Failed(event) => self.resource_events.push(event),
                DesktopResourceUpdate::Released {
                    resource, evict, ..
                } => {
                    if evict {
                        self.surface.release_raster_resource(resource);
                    }
                }
            }
        }
    }
}

fn drain_resource_commands(
    runtime: &RuntimeInstance,
    mut send: impl FnMut(whisker_protocol::ResourceCommand) -> Result<(), DesktopError>,
) -> Result<usize, DesktopError> {
    let commands = runtime.surface().take_resource_commands();
    let count = commands.len();
    for command in commands {
        send(command)?;
    }
    Ok(count)
}

fn dispatch_resource_events(
    runtime: &RuntimeInstance,
    events: Vec<whisker_protocol::ResourceEvent>,
) -> Result<bool, DesktopError> {
    let mut applied = false;
    for event in events {
        applied |= runtime
            .dispatch_resource_event(&event)
            .map_err(|error| DesktopError(format!("dispatch Desktop resource event: {error}")))?
            == whisker::ResourceEventApply::Applied;
    }
    Ok(applied)
}

fn validate_context(context: DesktopFrameContext) -> Result<(), DesktopError> {
    if !context.timestamp_ms.is_finite()
        || !context.logical_width.is_finite()
        || !context.logical_height.is_finite()
        || !context.scale.is_finite()
        || context.logical_width <= 0.0
        || context.logical_height <= 0.0
        || context.scale <= 0.0
    {
        return Err(DesktopError(
            "Desktop frame context must contain finite positive viewport metrics".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use whisker::SurfaceRuntime;
    use whisker::prelude::*;
    use whisker_protocol::{
        ResourceCommand, ResourceDimensions, ResourceEvent, ResourceId, ResourceKind,
        ResourceRequest, ResourceSource,
    };

    #[test]
    fn frame_context_rejects_invalid_host_metrics() {
        let valid = DesktopFrameContext {
            timestamp_ms: 1.0,
            logical_width: 100.0,
            logical_height: 80.0,
            scale: 2.0,
            environment_epoch: 1,
            viewport_epoch: 1,
        };
        assert!(validate_context(valid).is_ok());
        assert!(
            validate_context(DesktopFrameContext {
                scale: 0.0,
                ..valid
            })
            .is_err()
        );
        assert!(
            validate_context(DesktopFrameContext {
                timestamp_ms: f64::NAN,
                ..valid
            })
            .is_err()
        );
    }

    #[test]
    fn typed_resource_commands_and_events_cross_the_desktop_runtime_boundary() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let surface = SurfaceRuntime::new(
            SurfaceId::new(1).unwrap(),
            StyleEnvironment::new(100.0, 100.0, 1.0, 16.0),
        );
        let mut runtime = RuntimeInstance::new(
            surface,
            RuntimeWakeHandle::new(move || {
                wake_count.fetch_add(1, Ordering::SeqCst);
            }),
        );
        runtime.mount(|| render! { view() }).unwrap();
        let resource = ResourceId::new(7).unwrap();
        let command = ResourceCommand::Load(ResourceRequest {
            resource,
            generation: 1,
            kind: ResourceKind::RasterImage,
            source: ResourceSource::Bytes {
                media_type: "image/png".into(),
                data: vec![1, 2, 3],
            },
        });
        runtime
            .surface()
            .enqueue_resource_command(command.clone())
            .unwrap();

        let mut sent = Vec::new();
        assert_eq!(
            drain_resource_commands(&runtime, |command| {
                sent.push(command);
                Ok(())
            })
            .unwrap(),
            1
        );
        assert_eq!(sent, vec![command]);
        assert!(runtime.surface().take_resource_commands().is_empty());

        let event = ResourceEvent::Ready {
            resource,
            generation: 1,
            dimensions: Some(ResourceDimensions {
                width: 2.0,
                height: 2.0,
                scale: 1.0,
            }),
        };
        let wakes_before_event = wakes.load(Ordering::SeqCst);
        assert!(dispatch_resource_events(&runtime, vec![event.clone()]).unwrap());
        assert_eq!(runtime.surface().resource_event(resource, 1), Some(event));
        assert!(wakes.load(Ordering::SeqCst) > wakes_before_event);
    }
}
