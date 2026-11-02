//! Desktop Host implementation for Kiba.

use std::sync::OnceLock;

use kiba::SceneRenderer;
use whisker_desktop::{DesktopViewDefinition, ModuleDefinition, WhiskerModule};

const MODULE_NAME: &str = "kiba:Viewport";

struct ViewportModule;

#[WhiskerModule]
impl WhiskerModule for ViewportModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name(MODULE_NAME).view(
            DesktopViewDefinition::new(MODULE_NAME, |_| OnceLock::<SceneRenderer>::new()).gpu(
                |scene, frame| {
                    scene
                        .get_or_init(|| SceneRenderer::new(frame.device, frame.format))
                        .draw(frame.device, frame.queue, frame.target);
                },
            ),
        )
    }
}
