//! Desktop Host implementation for Kiba.

use std::sync::Mutex;

use kiba::{Mat4, SceneRenderer};
use whisker_desktop::{DesktopViewDefinition, ModuleDefinition, WhiskerModule};

const MODULE_NAME: &str = "kiba:Viewport";

struct ViewportModule;

#[WhiskerModule]
impl WhiskerModule for ViewportModule {
    type Definition = ModuleDefinition;

    fn definition() -> Self::Definition {
        ModuleDefinition::new().name(MODULE_NAME).view(
            DesktopViewDefinition::new(MODULE_NAME, |_| Mutex::<Option<SceneRenderer>>::new(None))
                .prop("game", |_, _| {}, |_| {})
                .gpu(|scene, frame| {
                    let mut scene = scene.lock().expect("scene lock poisoned");
                    scene
                        .get_or_insert_with(|| SceneRenderer::new(frame.device, frame.format))
                        .draw(frame.device, frame.queue, frame.target, &[Mat4::IDENTITY]);
                }),
        )
    }
}
