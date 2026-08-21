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
use whisker_engine::HostLayoutOptions;
use whisker_protocol::{ElementRegistration, SurfaceId};
use whisker_style::StyleEnvironment;

mod element;
mod gpu;
mod paint;
mod scene;
mod surface;
mod text;

#[cfg(test)]
#[path = "../tests/host_conformance/mod.rs"]
mod host_conformance_tests;

use element::DesktopElementRegistry;
pub use element::{
    DesktopElementFactory, DesktopElementModule, standard_desktop_element_factories,
    standard_desktop_element_modules,
};
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
pub struct DesktopHostError(String);

impl fmt::Display for DesktopHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for DesktopHostError {}

/// Host-owned state shared by all native Desktop OS adapters.
///
/// The OS crate owns the window and [`RuntimeInstance`]. This type owns the
/// corresponding measurement provider, retained Host projection, and GPU
/// surface. Calls remain direct Rust calls; no FFI or serialization is added.
pub struct DesktopHost {
    measurements: NativeTextHost,
    surface: DesktopSurface,
}

impl DesktopHost {
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
    ) -> Result<Self, DesktopHostError> {
        let elements = DesktopElementRegistry::bind(elements, element_factories)
            .map_err(|error| DesktopHostError(format!("bind Desktop elements: {error}")))?;
        let surface = DesktopSurface::new(target, physical_size, surface, elements.clone())
            .await
            .map_err(|error| DesktopHostError(format!("initialize Desktop renderer: {error}")))?;
        Ok(Self {
            measurements: NativeTextHost::new(elements),
            surface,
        })
    }

    /// Reconfigures the GPU surface after an OS resize notification.
    pub fn resize(&mut self, physical_size: [u32; 2]) {
        self.surface.resize(physical_size);
    }

    /// Runs measurement, layout, frame presentation, and native painting once.
    pub fn drive_frame(
        &mut self,
        runtime: &RuntimeInstance,
        context: DesktopFrameContext,
    ) -> Result<DesktopFrameResult, DesktopHostError> {
        validate_context(context)?;
        let environment = StyleEnvironment::new(
            context.logical_width,
            context.logical_height,
            context.scale,
            14.0,
        );
        let drive = runtime
            .drive_frame(
                context.timestamp_ms,
                environment,
                context.environment_epoch,
                context.viewport_epoch,
                &mut self.measurements,
                &mut self.surface,
                HostLayoutOptions::default(),
            )
            .map_err(|error| DesktopHostError(format!("drive Desktop frame: {error}")))?;
        self.surface
            .paint(
                &mut self.measurements,
                [context.logical_width, context.logical_height],
                context.scale,
            )
            .map_err(|error| DesktopHostError(format!("paint Desktop frame: {error}")))?;
        Ok(DesktopFrameResult {
            needs_frame: drive.needs_frame,
        })
    }
}

fn validate_context(context: DesktopFrameContext) -> Result<(), DesktopHostError> {
    if !context.timestamp_ms.is_finite()
        || !context.logical_width.is_finite()
        || !context.logical_height.is_finite()
        || !context.scale.is_finite()
        || context.logical_width <= 0.0
        || context.logical_height <= 0.0
        || context.scale <= 0.0
    {
        return Err(DesktopHostError(
            "Desktop frame context must contain finite positive viewport metrics".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
