//! A 3D scene graph for Whisker, drawn with wgpu.
//!
//! Scenes are written as components inside a [`SceneView`]:
//!
//! ```ignore
//! render! {
//!     SceneView(style: stage) {
//!         PerspectiveCamera(position: Vec3::new(0.0, 1.5, 4.0))
//!         DirectionalLight(intensity: 2.0)
//!         Mesh(geometry: cube, material: StandardMaterial::default())
//!     }
//! }
//! ```
//!
//! The same tree can be built and changed directly through [`core`], which those components are
//! built on. Coordinates follow glTF: right-handed, +Y up, in meters.

mod components;
pub mod core;
mod model;
mod raycast;
mod render;

pub use components::{
    AmbientLight, DirectionalLight, Group, Mesh, ModelView, PerspectiveCamera, PointLight,
    SceneView, use_frame, use_scene,
};
pub use core::{
    AlphaMode, Geometry, Light, Material, Node, Projection, Scene, Shadow, StandardMaterial,
    Texture, TextureError, TextureTransform, Transform,
};
pub use model::{AnimationOptions, AnimationPlayer, Model, ModelError};
pub use raycast::{Ray, SceneHit};

/// The vector types the API takes.
pub mod math {
    pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
}
