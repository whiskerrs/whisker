use whisker::runtime::RuntimeWakeHandle;
use whisker_engine::FrameSink;
use whisker_protocol::{ApplyResult, FramePacket, RenderCapabilities, ResourceId, SurfaceId};

use crate::accessibility::DesktopAccessibilitySnapshot;
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
        background_rgb: [u8; 3],
        surface: SurfaceId,
        elements: DesktopElementRegistry,
        event_wake: RuntimeWakeHandle,
    ) -> Result<Self, GpuError> {
        Ok(Self {
            scene: DesktopScene::new_with_wake(surface, elements, event_wake),
            gpu: GpuRenderer::new(target, physical_size, background_rgb).await?,
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

    pub(crate) fn release_raster_resource(&mut self, resource: ResourceId) {
        self.gpu.release_raster_resource(resource);
        self.scene.release_raster_resource(resource);
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

    pub(crate) fn cursor_at(
        &self,
        logical_position: [f32; 2],
    ) -> Option<whisker_protocol::CursorKeyword> {
        self.scene.cursor_at(logical_position)
    }

    pub(crate) fn hit_test(&self, logical_position: [f32; 2]) -> Option<whisker_protocol::NodeId> {
        self.scene.hit_test(logical_position)
    }

    pub(crate) fn scroll_at(&mut self, logical_position: [f32; 2], delta: [f32; 2]) -> bool {
        self.scene.scroll_at(logical_position, delta)
    }

    pub(crate) fn settle_scroll_at(&mut self, logical_position: [f32; 2]) -> bool {
        self.scene.settle_scroll_at(logical_position)
    }

    pub(crate) fn focus_text_input_at(&mut self, logical_position: [f32; 2]) -> bool {
        self.scene.focus_text_input_at(logical_position)
    }

    pub(crate) fn dispatch_text_input(&mut self, event: &crate::DesktopTextInputEvent) -> bool {
        self.scene.dispatch_text_input(event)
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        self.scene.selected_text()
    }

    pub(crate) fn focused_text_input_rect(&self) -> Option<whisker_protocol::LayoutRect> {
        self.scene.focused_text_input_rect()
    }

    pub(crate) fn advance_scroll_animations(&mut self, delta_ms: f32) -> bool {
        self.scene.advance_scroll_animations(delta_ms)
    }

    pub(crate) fn has_active_scroll_animations(&self) -> bool {
        self.scene.has_active_scroll_animations()
    }

    pub(crate) fn accessibility_snapshot(&self) -> DesktopAccessibilitySnapshot {
        self.scene.accessibility_snapshot()
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
