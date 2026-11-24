//! Components that build a [`Scene`] from `render!`.
//!
//! Only [`SceneView`] creates an element on screen; the others add a [`Node`] to the scene for as
//! long as they stay mounted.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use glam::{Quat, Vec2, Vec3};
use whisker::event::TouchEvent;
use whisker::prelude::*;
use whisker::runtime::view::{Element, create_phantom_element};
use whisker::{Callback, Children, Signal, Style};
use whisker_surface::{FrameInfo, SurfaceView};

use crate::core::{
    Environment, Fog, FrameSubscription, Geometry, Light, Material, Node, Projection, Scene,
    Shadow, TapHandler, ToneMapping, Transform, rotation_toward,
};
use crate::model::{AnimationOptions, AnimationPlayer, Model};
use crate::raycast::SceneHit;

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
    #[prop(default = ToneMapping::None.into())] tone_mapping: Signal<ToneMapping>,
    /// Scales lit colors before tone mapping.
    #[prop(default = 1.0.into())]
    exposure: Signal<f32>,
    #[prop(default = None.into())] fog: Signal<Option<Fog>>,
    /// Lights and reflects in every surface the light arriving from all around.
    #[prop(default = None.into())]
    environment: Signal<Option<Environment>>,
    #[prop(default = 1.0.into())] environment_intensity: Signal<f32>,
    /// Draws the environment behind the scene in place of `background`.
    #[prop(default = false.into())]
    environment_background: Signal<bool>,
    /// Shares a scene built with the imperative API; a new one is created otherwise.
    scene: Option<Scene>,
    /// Called for a tap that no `on_tap` of the node hit, or its ancestors, handled; `None` when
    /// the tap hit nothing.
    on_tap: Option<Callback<Option<SceneHit>>>,
    children: Children,
) -> Element {
    let scene = scene.clone().unwrap_or_default();
    let surface = scene.surface();
    let picked = scene.clone();
    let tap = move |event: TouchEvent| {
        let (Some(offset), Some(size)) = (event.offset, event.size) else {
            return;
        };
        let point = Vec2::new(offset.x as f32, offset.y as f32);
        let size = Vec2::new(size.width as f32, size.height as f32);
        let hit = picked
            .ray_from_view(point, size)
            .and_then(|ray| picked.raycast(ray).into_iter().next());
        if let Some(hit) = &hit {
            let mut node = Some(hit.node.clone());
            while let Some(current) = node {
                if let Some(handler) = current.on_tap() {
                    handler(hit.clone());
                    return;
                }
                node = current.parent();
            }
        }
        if let Some(on_tap) = on_tap {
            on_tap.run(hit);
        }
    };
    let target = scene.clone();
    effect(move || target.set_background(background.get()));
    let target = scene.clone();
    effect(move || target.set_tone_mapping(tone_mapping.get(), exposure.get()));
    let target = scene.clone();
    effect(move || target.set_fog(fog.get()));
    let target = scene.clone();
    effect(move || target.set_environment(environment.get(), environment_intensity.get()));
    let target = scene.clone();
    effect(move || target.set_environment_background(environment_background.get()));
    provide_context(SceneContext {
        parent: scene.root(),
        scene,
    });
    let fill: Style = Css::new().width(percent(100)).height(percent(100)).into();
    let projected = children.clone();
    render! {
        View(style: style.clone(), on_tap: tap) {
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
    #[prop(default = true.into())] cast_shadow: Signal<bool>,
    #[prop(default = true.into())] receive_shadow: Signal<bool>,
    /// Called when a tap on the enclosing `SceneView` hits this mesh first.
    on_tap: Option<Callback<SceneHit>>,
) -> Element {
    let (_, node) = mount_node("Mesh");
    bind_transform(&node, position, rotation, scale, visible);
    bind_shadow(&node, cast_shadow, receive_shadow);
    bind_tap(&node, on_tap);
    let target = node.clone();
    effect(move || target.set_mesh(Some((geometry.get(), material.get()))));
    create_phantom_element()
}

fn bind_shadow(node: &Node, cast: Signal<bool>, receive: Signal<bool>) {
    let target = node.clone();
    effect(move || target.set_cast_shadow(cast.get()));
    let target = node.clone();
    effect(move || target.set_receive_shadow(receive.get()));
}

fn bind_tap(node: &Node, on_tap: Option<Callback<SceneHit>>) {
    let Some(on_tap) = on_tap else {
        return;
    };
    let handler: TapHandler = Rc::new(move |hit| on_tap.run(hit));
    node.set_on_tap(Some(handler));
    let node = node.clone();
    on_cleanup(move || node.set_on_tap(None));
}

/// A glTF model, placed anew for this component and animated with its clips.
#[component]
pub fn model_view(
    model: Signal<Model>,
    #[prop(default = Vec3::ZERO.into())] position: Signal<Vec3>,
    #[prop(default = Quat::IDENTITY.into())] rotation: Signal<Quat>,
    #[prop(default = Vec3::ONE.into())] scale: Signal<Vec3>,
    #[prop(default = true.into())] visible: Signal<bool>,
    /// The clip to play, by name; the model rests in its default pose while `None`.
    #[prop(default = None.into())]
    animation: Signal<Option<String>>,
    #[prop(default = AnimationOptions::default().into())] animation_options: Signal<
        AnimationOptions,
    >,
    #[prop(default = true.into())] cast_shadow: Signal<bool>,
    #[prop(default = true.into())] receive_shadow: Signal<bool>,
    /// Called when a tap on the enclosing `SceneView` hits a part of this model first;
    /// [`SceneHit::node`] is the part.
    on_tap: Option<Callback<SceneHit>>,
) -> Element {
    let (context, node) = mount_node("ModelView");
    bind_transform(&node, position, rotation, scale, visible);
    bind_shadow(&node, cast_shadow, receive_shadow);
    bind_tap(&node, on_tap);
    let player = signal(None::<AnimationPlayer>);
    let parent = node.clone();
    effect(move || {
        for child in parent.children() {
            child.remove_from_parent();
        }
        let root = model.get().instantiate();
        parent.add_child(&root);
        player.set(AnimationPlayer::new(&root));
    });
    let driver = Rc::new(AnimationDriver::default());
    let scene = context.scene;
    effect(move || {
        let (clip, options) = (animation.get(), animation_options.get());
        let Some(player) = player.get() else {
            return;
        };
        match &clip {
            Some(clip) => {
                player.play(clip, options);
            }
            None => player.stop(options.fade),
        }
        driver.run(&scene, player);
    });
    create_phantom_element()
}

/// Advances an [`AnimationPlayer`] every frame while it has clips playing or fading out.
#[derive(Default)]
pub(crate) struct AnimationDriver {
    subscription: Rc<RefCell<Option<FrameSubscription>>>,
}

impl AnimationDriver {
    pub(crate) fn run(&self, scene: &Scene, player: AnimationPlayer) {
        let slot = Rc::downgrade(&self.subscription);
        let last: Cell<Option<Duration>> = Cell::new(None);
        let subscription = scene.on_frame(move |frame| {
            let delta = last.replace(Some(frame.timestamp)).map_or(0.0, |previous| {
                frame.timestamp.saturating_sub(previous).as_secs_f32()
            });
            // A long gap, such as a backgrounded app, resumes instead of skipping ahead.
            player.update(delta.min(0.25));
            if !player.is_playing()
                && let Some(slot) = slot.upgrade()
            {
                slot.borrow_mut().take();
            }
        });
        *self.subscription.borrow_mut() = Some(subscription);
    }
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
    #[prop(default = None.into())] shadow: Signal<Option<Shadow>>,
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
            shadow: shadow.get(),
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
    /// Rendered as six maps, so it costs six times a directional light's single cascade.
    #[prop(default = None.into())]
    shadow: Signal<Option<Shadow>>,
) -> Element {
    let (_, node) = mount_node("PointLight");
    let target = node.clone();
    effect(move || target.set_transform(Transform::from_translation(position.get())));
    effect(move || {
        node.set_light(Some(Light::Point {
            color: color.get(),
            intensity: intensity.get(),
            range: range.get(),
            shadow: shadow.get(),
        }));
    });
    create_phantom_element()
}

/// A point light narrowed to a cone along `direction`, full inside `inner_angle` and fading out by
/// `outer_angle`, both in radians from the cone's axis.
#[component]
pub fn spot_light(
    #[prop(default = Color::rgb(255, 255, 255).into())] color: Signal<Color>,
    #[prop(default = 1.0.into())] intensity: Signal<f32>,
    #[prop(default = 10.0.into())] range: Signal<f32>,
    #[prop(default = Vec3::ZERO.into())] position: Signal<Vec3>,
    #[prop(default = Vec3::NEG_Y.into())] direction: Signal<Vec3>,
    #[prop(default = 0.0.into())] inner_angle: Signal<f32>,
    #[prop(default = std::f32::consts::FRAC_PI_4.into())] outer_angle: Signal<f32>,
    #[prop(default = None.into())] shadow: Signal<Option<Shadow>>,
) -> Element {
    let (_, node) = mount_node("SpotLight");
    let target = node.clone();
    effect(move || {
        target.set_transform(Transform {
            translation: position.get(),
            rotation: rotation_toward(direction.get(), Vec3::Y),
            scale: Vec3::ONE,
        });
    });
    effect(move || {
        node.set_light(Some(Light::Spot {
            color: color.get(),
            intensity: intensity.get(),
            range: range.get(),
            inner_angle: inner_angle.get(),
            outer_angle: outer_angle.get(),
            shadow: shadow.get(),
        }));
    });
    create_phantom_element()
}

/// Light from every direction, `sky_color` from above blending into `ground_color` from below.
#[component]
pub fn hemisphere_light(
    #[prop(default = Color::rgb(255, 255, 255).into())] sky_color: Signal<Color>,
    #[prop(default = Color::rgb(80, 80, 80).into())] ground_color: Signal<Color>,
    #[prop(default = 0.5.into())] intensity: Signal<f32>,
) -> Element {
    let scene = scene_context("HemisphereLight").scene;
    let target = scene.clone();
    effect(move || {
        target.set_hemisphere_light(sky_color.get(), ground_color.get(), intensity.get())
    });
    on_cleanup(move || {
        scene.set_hemisphere_light(Color::rgb(255, 255, 255), Color::rgb(0, 0, 0), 0.0)
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
