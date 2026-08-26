use whisker_engine::FrameSink;
use whisker_protocol::{ApplyResult, FramePacket, RenderCapabilities, ResourceId, SurfaceId};

use crate::element::DesktopElementRegistry;
use crate::gpu::{GpuError, GpuRenderer, RasterResource};
use crate::scene::DesktopProviderEvent;
use crate::scene::{DesktopPresentError, DesktopScene};
use crate::text::NativeTextHost;

/// The complete Host-owned projection and presentation state for one native
/// window surface.
pub(crate) struct DesktopSurface {
    scene: DesktopScene,
    gpu: GpuRenderer,
}

impl DesktopSurface {
    pub(crate) async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        physical_size: [u32; 2],
        surface: SurfaceId,
        elements: DesktopElementRegistry,
    ) -> Result<Self, GpuError> {
        Ok(Self {
            scene: DesktopScene::new(surface, elements),
            gpu: GpuRenderer::new(target, physical_size).await?,
        })
    }

    pub(crate) fn resize(&mut self, physical_size: [u32; 2]) {
        self.gpu.resize(physical_size);
    }

    pub(crate) fn register_raster_resource(
        &mut self,
        resource: ResourceId,
        raster: &RasterResource,
    ) {
        self.gpu.register_raster_resource(resource, raster);
        self.scene.register_raster_resource(resource);
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

    pub(crate) fn take_events(&mut self) -> Vec<DesktopProviderEvent> {
        self.scene.take_events()
    }
}

impl FrameSink for DesktopSurface {
    type Error = DesktopPresentError;

    fn capabilities(&self) -> RenderCapabilities {
        self.scene.capabilities()
    }

    fn present(&mut self, packet: &FramePacket) -> Result<ApplyResult, Self::Error> {
        self.scene.present(packet)
    }
}
