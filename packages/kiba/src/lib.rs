//! 3D game engine for Whisker.
//!
//! A game is built in ordinary Rust inside [`Game::new`] and shown with the [`Viewport`] element.
//! Entities live in a [`hecs`] [`World`]; these component sets make up a scene:
//!
//! | To get | Spawn |
//! |---|---|
//! | A view | `(Camera::default(), Transform::looking_at(eye, target, Vec3::Y))` |
//! | A view that blurs what is out of focus | `Camera { depth_of_field: Some(DepthOfField { focus_distance, ..DepthOfField::default() }), ..Camera::default() }` |
//! | Direct light and shadows | `(Sun::default(),)` |
//! | A lamp or torch, without shadows; the nearest 16 in view shade | `(Transform::at(position), PointLight::default())` |
//! | A flashlight along -Z, without shadows | `(Transform::looking_at(position, target, Vec3::Y), SpotLight::default())` |
//! | Reflections of a room or street on glossy surfaces inside it | `(Transform::at(center), ReflectionProbe::new(size))` |
//! | A background and light from all around | `(Sky::default(),)`, or `Sky::from_sun(direction)` for a time of day |
//! | Haze that hides the distance | `(Fog::default(),)` |
//! | Fire, sparks, or magic | `(Transform::default(), ParticleEmitter::default())` |
//! | A shape | `(Transform::default(), Mesh::cube(1.0), Material::default())` |
//! | Glass, leaves, or a glowing sign | That `Material` with `alpha_mode: AlphaMode::Blend` and `opacity`, `AlphaMode::Mask { cutoff: 0.5 }` and `double_sided: true`, or `emissive` |
//! | A file | `(Transform::default(), Model::load(asset!("crate.glb")))` |
//! | Sky light blocked and bounced by still models | `(ProbeVolume::load(asset!("scene.kiba")),)`, cooked with `kiba-cook` |
//! | A large world, loaded around the camera | `(WorldStream::load(asset!("world/index.kiba"), StreamSettings::default()),)`, cooked with `kiba-cook` |
//! | Sound | `Sound::load(asset!("step.ogg"))`, held by a callback, then `.play()` |
//! | Physics, with the `physics` feature | `(Physics::default(),)` once, then `(Transform::default(), RigidBody::new(BodyKind::Dynamic), Collider::Ball(0.5))` |
//! | A file's animation | Add `Animator::new("walk")` next to its `Model` |
//!
//! `asset!` is not part of Kiba. It comes from the `whisker-asset` crate, as `whisker_asset::asset!`, and
//! checks at compile time that the file exists in your crate's `assets/` directory.
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
//!
//! Rendering quality adapts by itself: a game starts at [`Tier::T2`] and lowers resolution, then the
//! tier, while frames take too long. [`FrameStats`] shows where it settled, and
//! [`GameSetup::set_tier`] or [`Context::set_tier`] fixes the tier.
//!
//! # Recipes
//!
//! A character that moves in fixed steps, with a camera that follows what is drawn. `spawn` returns
//! the [`Entity`], which callbacks can capture because it is `Copy`:
//!
//! ```ignore
//! let hero = setup.world.spawn((
//!     Transform::default(),
//!     Interpolated::default(),
//!     Model::load(asset!("hero.glb")),
//!     Animator::new("idle"),
//! ));
//! let camera = setup.world.spawn((Camera::default(), Transform::default()));
//! setup.bind_action("forward", &[Key::W, Key::Up]);
//! setup.bind_action("jump", &[Key::Space]);
//! setup.on_fixed_update(move |context| {
//!     let walking = context.input.action_down("forward");
//!     let step = if walking { 2.0 * context.time.delta_seconds() } else { 0.0 };
//!     let mut hero = context.world.query_one_mut::<(&mut Transform, &mut Animator)>(hero).unwrap();
//!     hero.0.translation.z -= step;
//!     let clip = if walking { "walk" } else { "idle" };
//!     if hero.1.clip() != clip {
//!         hero.1.play(clip, 0.2);
//!     }
//! });
//! setup.on_update(move |context| {
//!     let Some(drawn) = context.drawn_transform(hero) else { return };
//!     let target = drawn.translation;
//!     let eye = target + Vec3::new(0.0, 2.0, 5.0);
//!     let view = Transform::looking_at(eye, target, Vec3::Y);
//!     *context.world.query_one_mut::<&mut Transform>(camera).unwrap() = view;
//! });
//! ```
//!
//! A HUD over the game and a panel below it, with signals carrying state out of the game and
//! commands carrying taps in. The viewport draws at whatever size layout gives it, and a later sibling
//! draws over it. `PositionKind` comes from `whisker::css`:
//!
//! ```ignore
//! let score = signal(0);
//! let game = Game::<Command>::new(move |setup| {
//!     setup.on_command(|context, Command::Reset| { /* change components */ });
//!     let mut shown = 0;
//!     setup.on_update(move |context| {
//!         let now = /* read components */ 0;
//!         if now != shown {
//!             shown = now;
//!             score.set(now);
//!         }
//!     });
//! });
//! render! {
//!     View(style: css!(width: percent(100), height: percent(100), flex_direction: FlexDirection::Column)) {
//!         View(style: css!(width: percent(100), height: percent(70))) {
//!             Viewport(game: game, style: css!(width: percent(100), height: percent(100)))
//!             Text(value: "HUD", style: css!(position: PositionKind::Absolute, top: px(12), left: px(12)))
//!         }
//!         View(style: css!(flex_grow: 1.0), on_tap: move |_| game.send(Command::Reset)) {
//!             Text(value: computed(move || format!("Score {}", score.get())))
//!         }
//!     }
//! }
//! ```
//!
//! # What Kiba leaves to the game
//!
//! Kiba has no navmesh and no networking. For pathfinding, search a grid of your own or use a crate such
//! as `pathfinding`; `Physics::raycast`, with the `physics` feature, checks a line of sight but cannot route around
//! corners. For networking, build snapshots in `on_fixed_update`, whose step is exact, and give entities
//! an id component of your own, because an [`Entity`] means nothing outside its world. Simulation is not
//! promised to be deterministic across machines: it uses `f32` math, and query order is not specified.
//!
//! An on-screen button that holds the action, with the touch events of Whisker's elements:
//!
//! ```ignore
//! View(
//!     on_touchstart: move |_| game.set_action("jump", true),
//!     on_touchend: move |_| game.set_action("jump", false),
//!     on_touchcancel: move |_| game.set_action("jump", false),
//! ) { Text(value: "Jump") }
//! ```

mod animation;
mod audio;
mod controller;
#[cfg(feature = "cook")]
pub mod cook;
mod game;
mod input;
mod lights;
mod mesh;
mod model;
mod pack;
mod particles;
#[cfg(feature = "physics")]
mod physics;
mod post;
mod probes;
mod quality;
mod reflection;
mod renderer;
mod sky;
mod stream;
mod texture;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod ios;

pub use animation::Animator;
#[doc(hidden)]
pub use audio::resume_audio;
pub use audio::{Playback, Sound};
pub use controller::{FlyCamera, OrbitCamera};
pub use game::{
    Camera, Context, DepthOfField, FrameStats, Game, GameSetup, Interpolated, Sun, Time, Transform,
};
#[doc(hidden)]
pub use game::{attach_renderer, detach_renderer, send_input};
pub use glam::{Mat4, Quat, Vec2, Vec3};
pub use hecs::{Entity, World};
#[doc(hidden)]
pub use input::InputEvent;
pub use input::{Input, Key, Pointer};
pub use lights::{PointLight, SpotLight};
pub use mesh::{AlphaMode, Material, Mesh, Vertex};
#[cfg(feature = "gltf")]
pub use model::ModelError;
pub use model::{LoadState, Model};
pub use particles::{ParticleEmitter, Sprites};
#[cfg(feature = "physics")]
pub use physics::{BodyKind, Collider, Physics, RayHit, RigidBody};
pub use probes::ProbeVolume;
pub use quality::Tier;
pub use reflection::ReflectionProbe;
pub use renderer::{Frame, Renderer, SceneRenderer};
#[doc(hidden)]
pub use renderer::{log, placeholder_scene};
pub use sky::{Fog, Sky};
pub use stream::{StreamSettings, WorldStream};
pub use texture::Texture;
#[doc(hidden)]
pub use wgpu;

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        AlphaMode, Animator, Camera, Context, DepthOfField, Entity, FlyCamera, Fog, FrameStats,
        Game, GameSetup, Input, Interpolated, Key, LoadState, Material, Mesh, Model, OrbitCamera,
        ParticleEmitter, Playback, PointLight, Pointer, ProbeVolume, Quat, ReflectionProbe, Sky,
        Sound, SpotLight, StreamSettings, Sun, Texture, Tier, Time, Transform, Vec2, Vec3, Vertex,
        Viewport, World, WorldStream,
    };
    #[cfg(feature = "physics")]
    pub use crate::{BodyKind, Collider, Physics, RayHit, RigidBody};
}

/// A surface that shows a [`Game`]: `Viewport(game: game, style: css!(...))`, where `game` converts
/// into the `Signal<String>` the element carries.
///
/// Give it a size through `style`; it draws at its layout size every frame, so it follows resizes. Layer
/// UI over it as later siblings positioned with `PositionKind::Absolute`. Keys reach the game while
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
