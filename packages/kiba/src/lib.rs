//! 3D game engine for Whisker.
//!
//! Application code mounts [`Viewport`]; each Host only supplies a render target
//! and frame timing to the shared wgpu [`SceneRenderer`].

mod game;
mod mesh;
mod renderer;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod ios;

pub use game::{Camera, Game, GameSetup, Time, Transform};
#[doc(hidden)]
pub use game::{attach_renderer, detach_renderer};
pub use glam::{Mat4, Quat, Vec3};
pub use hecs::{Entity, World};
pub use mesh::{Mesh, Vertex};
#[doc(hidden)]
pub use renderer::placeholder_scene;
pub use renderer::{Batch, Renderer, SceneRenderer};
#[doc(hidden)]
pub use wgpu;

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        Camera, Entity, Game, GameSetup, Mesh, Quat, Time, Transform, Vec3, Vertex, Viewport, World,
    };
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
