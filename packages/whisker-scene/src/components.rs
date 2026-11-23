//! Components that build a [`Scene`] from `render!`.
//!
//! Only [`SceneView`] creates an element on screen; the others add a [`Node`] to the scene for as
//! long as they stay mounted.

use glam::{Quat, Vec3};
use whisker::prelude::*;
use whisker::runtime::view::{Element, create_phantom_element};
use whisker::{Children, Signal, Style};
use whisker_surface::{FrameInfo, SurfaceView};

use crate::core::{Geometry, Light, Material, Node, Projection, Scene, Transform, rotation_toward};

#[derive(Clone)]
struct SceneContext {
    scene: Scene,
    parent: Node,
}

fn scene_context(component: &str) -> SceneContext {
    use_context::<SceneContext>()
        .unwrap_or_else(|| panic!("{component} must be placed inside a SceneView"))
}

/// The scene of the nearest enclosing [`SceneView`].
///
/// # Panics
///
/// Outside a `SceneView`, or when called from an event handler instead of while rendering.
pub fn use_scene() -> Scene {
    scene_context("use_scene").scene
}

/// Calls `callback` at the start of every frame while the calling component stays mounted.
///
/// The scene draws continuously while any callback is registered.
///
/// # Panics
///
/// Outside a `SceneView`, or when called from an event handler instead of while rendering.
pub fn use_frame(callback: impl FnMut(&FrameInfo) + 'static) {
    let subscription = scene_context("use_frame").scene.on_frame(callback);
    on_cleanup(move || drop(subscription));
}

/// Adds a child node of the current parent for this component's lifetime.
fn mount_node(component: &str) -> (SceneContext, Node) {
    let context = scene_context(component);
    let node = Node::new();
    context.parent.add_child(&node);
    let mounted = node.clone();
    on_cleanup(move || mounted.remove_from_parent());
    (context, node)
}

fn bind_transform(
    node: &Node,
    position: Signal<Vec3>,
    rotation: Signal<Quat>,
    scale: Signal<Vec3>,
    visible: Signal<bool>,
) {
    let target = node.clone();
    effect(move || {
        target.set_transform(Transform {
            translation: position.get(),
            rotation: rotation.get(),
            scale: scale.get(),
        });
    });
    let target = node.clone();
    effect(move || target.set_visible(visible.get()));
}

/// Shows a 3D scene in the element's box.
///
/// The scene's nodes come from the components among `children`.
#[component]
pub fn scene_view(
    style: Style,
    #[prop(default = Color::rgb(0, 0, 0).into())] background: Signal<Color>,
    /// Shares a scene built with the imperative API; a new one is created otherwise.
    scene: Option<Scene>,
    children: Children,
) -> Element {
    let scene = scene.clone().unwrap_or_default();
    let surface = scene.surface();
    let target = scene.clone();
    effect(move || target.set_background(background.get()));
    provide_context(SceneContext {
        parent: scene.root(),
        scene,
    });
    let fill: Style = Css::new().width(percent(100)).height(percent(100)).into();
    let projected = children.clone();
    render! {
        View(style: style.clone()) {
            SurfaceView(surface: surface, style: fill)
            Fragment { { projected() } }
        }
    }
}

/// A node that moves its children together.
#[component]
pub fn group(
    #[prop(default = Vec3::ZERO.into())] position: Signal<Vec3>,
    #[prop(default = Quat::IDENTITY.into())] rotation: Signal<Quat>,
    #[prop(default = Vec3::ONE.into())] scale: Signal<Vec3>,
    #[prop(default = true.into())] visible: Signal<bool>,
    children: Children,
) -> Element {
    let (context, node) = mount_node("Group");
    bind_transform(&node, position, rotation, scale, visible);
    provide_context(SceneContext {
        scene: context.scene,
        parent: node,
    });
    let projected = children.clone();
    render! { Fragment { { projected() } } }
}

/// A shape drawn with a material.
#[component]
pub fn mesh(
    geometry: Signal<Geometry>,
    material: Signal<Material>,
    #[prop(default = Vec3::ZERO.into())] position: Signal<Vec3>,
    #[prop(default = Quat::IDENTITY.into())] rotation: Signal<Quat>,
    #[prop(default = Vec3::ONE.into())] scale: Signal<Vec3>,
    #[prop(default = true.into())] visible: Signal<bool>,
) -> Element {
    let (_, node) = mount_node("Mesh");
    bind_transform(&node, position, rotation, scale, visible);
    let target = node.clone();
    effect(move || target.set_mesh(Some((geometry.get(), material.get()))));
    create_phantom_element()
}

/// The viewpoint the scene is drawn from.
///
/// With several cameras, the last one to become `active` is used.
#[component]
pub fn perspective_camera(
    /// Vertical field of view in radians.
    #[prop(default = 60f32.to_radians().into())]
    fov_y: Signal<f32>,
    #[prop(default = 0.1.into())] near: Signal<f32>,
    #[prop(default = 1000.0.into())] far: Signal<f32>,
    #[prop(default = Vec3::new(0.0, 0.0, 5.0).into())] position: Signal<Vec3>,
    #[prop(default = Vec3::ZERO.into())] look_at: Signal<Vec3>,
    #[prop(default = true.into())] active: Signal<bool>,
) -> Element {
    let (context, node) = mount_node("PerspectiveCamera");
    let target = node.clone();
    effect(move || {
        target.set_transform(Transform::looking_at(
            position.get(),
            look_at.get(),
            Vec3::Y,
        ));
    });
    let target = node.clone();
    effect(move || {
        target.set_camera(Some(Projection::Perspective {
            fov_y: fov_y.get(),
            near: near.get(),
            far: far.get(),
        }));
    });
    let (scene, target) = (context.scene.clone(), node.clone());
    effect(move || {
        if active.get() {
            scene.set_active_camera(Some(&target));
        } else if scene.active_camera().as_ref() == Some(&target) {
            scene.set_active_camera(None);
        }
    });
    let scene = context.scene;
    on_cleanup(move || {
        if scene.active_camera().as_ref() == Some(&node) {
            scene.set_active_camera(None);
        }
    });
    create_phantom_element()
}

/// Parallel light shining along `direction`, like sunlight.
#[component]
pub fn directional_light(
    #[prop(default = Color::rgb(255, 255, 255).into())] color: Signal<Color>,
    #[prop(default = 1.0.into())] intensity: Signal<f32>,
    #[prop(default = Vec3::new(-0.5, -1.0, -0.3).into())] direction: Signal<Vec3>,
) -> Element {
    let (_, node) = mount_node("DirectionalLight");
    let target = node.clone();
    effect(move || {
        target.set_transform(Transform {
            rotation: rotation_toward(direction.get(), Vec3::Y),
            ..Transform::IDENTITY
        });
    });
    effect(move || {
        node.set_light(Some(Light::Directional {
            color: color.get(),
            intensity: intensity.get(),
        }));
    });
    create_phantom_element()
}

/// Light from a point that fades out by `range`.
#[component]
pub fn point_light(
    #[prop(default = Color::rgb(255, 255, 255).into())] color: Signal<Color>,
    #[prop(default = 1.0.into())] intensity: Signal<f32>,
    #[prop(default = 10.0.into())] range: Signal<f32>,
    #[prop(default = Vec3::ZERO.into())] position: Signal<Vec3>,
) -> Element {
    let (_, node) = mount_node("PointLight");
    let target = node.clone();
    effect(move || target.set_transform(Transform::from_translation(position.get())));
    effect(move || {
        node.set_light(Some(Light::Point {
            color: color.get(),
            intensity: intensity.get(),
            range: range.get(),
        }));
    });
    create_phantom_element()
}

/// Light that reaches every surface equally, from no direction.
#[component]
pub fn ambient_light(
    #[prop(default = Color::rgb(255, 255, 255).into())] color: Signal<Color>,
    #[prop(default = 0.1.into())] intensity: Signal<f32>,
) -> Element {
    let scene = scene_context("AmbientLight").scene;
    let target = scene.clone();
    effect(move || target.set_ambient_light(color.get(), intensity.get()));
    on_cleanup(move || scene.set_ambient_light(Color::rgb(255, 255, 255), 0.0));
    create_phantom_element()
}

#[cfg(test)]
mod tests;
