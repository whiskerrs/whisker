use std::sync::Arc;

use whisker_engine::FrameSink;
use whisker_protocol::{ApplyResult, FramePacket, SurfaceId, ValidationError};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::gpu::{GpuError, GpuRenderer};
use crate::scene::DesktopScene;
use crate::text::NativeTextHost;

/// The complete Host-owned projection and presentation state for one native
/// window surface.
pub(crate) struct DesktopSurface {
    scene: DesktopScene,
    gpu: GpuRenderer,
}

impl DesktopSurface {
    pub(crate) async fn new(window: Arc<Window>, surface: SurfaceId) -> Result<Self, GpuError> {
        Ok(Self {
            scene: DesktopScene::new(surface),
            gpu: GpuRenderer::new(window).await?,
        })
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        self.gpu.resize(size);
    }

    pub(crate) fn paint(
        &mut self,
        text: &mut NativeTextHost,
        logical_size: [f32; 2],
        scale: f32,
    ) -> Result<(), GpuError> {
        self.gpu
            .render(&self.scene.paint_commands(), text, logical_size, scale)
    }
}

impl FrameSink for DesktopSurface {
    type Error = ValidationError;

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        self.scene.present(packet)
    }
}
