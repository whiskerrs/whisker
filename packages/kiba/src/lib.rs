//! 3D game engine for Whisker.
//!
//! A game is built in ordinary Rust inside [`Game::new`] and shown with the [`Viewport`] element.
//! Entities live in a [`hecs`] [`World`]; these component sets make up a scene:
//!
//! | To get | Spawn |
//! |---|---|
//! | A view | `(Camera::default(), Transform::looking_at(eye, target, Vec3::Y))` |
//! | Light | `(Sun::default(),)` |
//! | A shape | `(Transform::default(), Mesh::cube(1.0), Material::default())` |
//! | A file | `(Transform::default(), Model::load(asset!("crate.glb")))` |
//!
//! Your own structs are components too, and mutable game state belongs in them, because each
//! callback owns what it captures. Query them from callbacks with `context.world.query_mut`.
//!
//! ```ignore
//! use kiba::prelude::*;
//! use whisker::prelude::*;
//! use whisker::runtime::view::Element;
//!
//! #[whisker::main]
//! pub fn app() -> Element {
//!     let game = Game::<()>::new(|setup| {
//!         setup.world.spawn((
//!             Camera::default(),
//!             Transform::looking_at(Vec3::new(0.0, 1.0, 3.0), Vec3::ZERO, Vec3::Y),
//!         ));
//!         setup.world.spawn((Sun::default(),));
//!         setup.world.spawn((Transform::default(), Mesh::cube(1.0)));
//!         setup.on_update(|context| {
//!             let angle = context.time.elapsed_seconds() as f32;
//!             for (_, (transform, _)) in context.world.query_mut::<(&mut Transform, &Mesh)>() {
//!                 transform.rotation = Quat::from_rotation_y(angle);
//!             }
//!         });
//!     });
//!     render! {
//!         Viewport(game: game, style: css!(width: percent(100), height: percent(100)))
//!     }
//! }
//! ```
//!
//! The camera looks along -Z, with +Y up and +X to its right.

mod controller;
mod game;
mod input;
mod mesh;
mod model;
mod renderer;
mod sky;
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
pub use sky::Sky;
pub use texture::Texture;
#[doc(hidden)]
pub use wgpu;

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        Camera, Context, Entity, FlyCamera, FrameStats, Game, GameSetup, Input, Interpolated, Key,
        LoadState, Material, Mesh, Model, OrbitCamera, Pointer, Quat, Sky, Sun, Texture, Time,
        Transform, Vec2, Vec3, Vertex, Viewport, World,
    };
}

/// A surface that shows a [`Game`]: `Viewport(game: game, style: css!(...))`.
///
/// Give it a size through `style`, and layer UI over it as its siblings. Keys reach the game while
/// no text field has focus.
#[whisker::module_element(name = "kiba:Viewport", measurement = None)]
pub fn viewport(game: whisker::Signal<String>, style: whisker::Style) {}

#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        [viewport_schema::element_provider()],
    )
}
