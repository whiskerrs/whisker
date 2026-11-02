//! 3D game engine for Whisker.
//!
//! Application code mounts [`Viewport`]; each Host only supplies a render target
//! and frame timing to the shared wgpu [`SceneRenderer`].

mod game;
mod renderer;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod ios;

pub use game::{App, Game, Time, Transform, Triangle};
#[doc(hidden)]
pub use game::{attach_renderer, detach_renderer};
pub use glam::{Mat4, Quat, Vec3};
pub use hecs::{Entity, World};
pub use renderer::{Renderer, SceneRenderer};
#[doc(hidden)]
pub use wgpu;

/// Commonly used items.
pub mod prelude {
    pub use crate::{App, Entity, Game, Quat, Time, Transform, Triangle, Vec3, Viewport, World};
}

/// A surface that shows a [`Game`].
#[whisker::module_element(name = "kiba:Viewport", measurement = None)]
pub fn viewport(game: whisker::Signal<String>, style: whisker::Style) {}

#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [viewport_schema::element_provider()],
    )
}
