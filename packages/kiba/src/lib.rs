//! 3D game engine for Whisker.
//!
//! Application code mounts [`Viewport`]; each Host only supplies a render target
//! and frame timing to the shared wgpu [`SceneRenderer`].

mod controller;
mod game;
mod input;
mod mesh;
mod model;
mod renderer;
mod texture;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod ios;

pub use controller::{FlyCamera, OrbitCamera};
pub use game::{Camera, Context, FrameStats, Game, GameSetup, Interpolated, Sun, Time, Transform};
#[doc(hidden)]
pub use game::{attach_renderer, detach_renderer, send_input};
pub use glam::{Mat4, Quat, Vec2, Vec3};
pub use hecs::{Entity, World};
#[doc(hidden)]
pub use input::InputEvent;
pub use input::{Input, Key, Pointer};
pub use mesh::{Material, Mesh, Vertex};
#[cfg(feature = "gltf")]
pub use model::ModelError;
pub use model::{LoadState, Model};
pub use renderer::{Frame, Renderer, SceneRenderer};
#[doc(hidden)]
pub use renderer::{log, placeholder_scene};
pub use texture::Texture;
#[doc(hidden)]
pub use wgpu;

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        Camera, Context, Entity, FlyCamera, FrameStats, Game, GameSetup, Input, Interpolated, Key,
        LoadState, Material, Mesh, Model, OrbitCamera, Pointer, Quat, Sun, Texture, Time,
        Transform, Vec2, Vec3, Vertex, Viewport, World,
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
