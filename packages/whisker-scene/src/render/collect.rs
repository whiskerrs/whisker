//! Gathering what a frame draws from the node tree.

use glam::{Mat4, Vec3};
use std::rc::Rc;

use crate::core::{
    Deform, Geometry, Instances, Light, Material, Node, Projection, Scene, Shadow, Transform,
    linear_rgba,
};

/// One mesh to draw.
pub(crate) struct Draw {
    pub(crate) model: Mat4,
    pub(crate) geometry: Geometry,
    pub(crate) material: Material,
    pub(crate) deform: Option<Rc<Deform>>,
    pub(crate) instances: Option<Instances>,
    pub(crate) cast_shadow: bool,
    pub(crate) receive_shadow: bool,
}

pub(crate) struct DirectionalLight {
    /// Unit vector from a surface toward the light.
    pub(crate) toward: Vec3,
    pub(crate) radiance: [f32; 4],
    pub(crate) shadow: Option<Shadow>,
}

/// A point light, or a spot light when it has a cone.
pub(crate) struct PointLight {
    pub(crate) position: Vec3,
    pub(crate) range: f32,
    pub(crate) radiance: [f32; 4],
    pub(crate) shadow: Option<Shadow>,
    pub(crate) cone: Option<Cone>,
}

#[derive(Clone, Copy)]
pub(crate) struct Cone {
    /// Unit vector the light shines along.
    pub(crate) direction: Vec3,
    pub(crate) inner_angle: f32,
    pub(crate) outer_angle: f32,
}

/// What one frame draws, gathered from the node tree.
#[derive(Default)]
pub(crate) struct Collected {
    pub(crate) draws: Vec<Draw>,
    pub(crate) directional: Vec<DirectionalLight>,
    pub(crate) point: Vec<PointLight>,
}

/// What the tree draws as seen from `eye`, a world-space camera position.
pub(crate) fn collect(root: &Node, eye: Vec3) -> Collected {
    let mut collected = Collected::default();
    visit(root, Mat4::IDENTITY, eye, (true, true), &mut collected);
    collected
}

/// Whether `node`, placed at `world`, shows from `eye` given its visible distance.
pub(crate) fn within_distance(node: &Node, world: Mat4, eye: Vec3) -> bool {
    node.visible_distance()
        .is_none_or(|range| range.contains(&world.transform_point3(Vec3::ZERO).distance(eye)))
}

fn visit(node: &Node, parent: Mat4, eye: Vec3, (cast, receive): (bool, bool), out: &mut Collected) {
    if !node.visible() {
        return;
    }
    let world = parent * node.transform().to_matrix();
    if !within_distance(node, world, eye) {
        return;
    }
    let (cast, receive) = (cast && node.cast_shadow(), receive && node.receive_shadow());
    if let Some((geometry, material)) = node.mesh() {
        let deform = node.deform();
        let model = match &deform {
            Some(deform) if deform.is_skinned() => Mat4::IDENTITY,
            _ => world,
        };
        out.draws.push(Draw {
            model,
            geometry,
            material,
            deform,
            instances: node.instances(),
            cast_shadow: cast,
            receive_shadow: receive,
        });
    }
    match node.light() {
        Some(Light::Directional {
            color,
            intensity,
            shadow,
        }) => {
            out.directional.push(DirectionalLight {
                toward: -world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::Y),
                radiance: radiance(color, intensity),
                shadow,
            });
        }
        Some(Light::Point {
            color,
            intensity,
            range,
            shadow,
        }) => {
            out.point.push(PointLight {
                position: world.transform_point3(Vec3::ZERO),
                range,
                radiance: radiance(color, intensity),
                shadow,
                cone: None,
            });
        }
        Some(Light::Spot {
            color,
            intensity,
            range,
            inner_angle,
            outer_angle,
            shadow,
        }) => {
            out.point.push(PointLight {
                position: world.transform_point3(Vec3::ZERO),
                range,
                radiance: radiance(color, intensity),
                shadow,
                cone: Some(Cone {
                    direction: world
                        .transform_vector3(Vec3::NEG_Z)
                        .normalize_or(Vec3::NEG_Z),
                    inner_angle,
                    outer_angle,
                }),
            });
        }
        None => {}
    }
    for child in node.children() {
        visit(&child, world, eye, (cast, receive), out);
    }
}

pub(super) fn radiance(color: whisker::css::Color, intensity: f32) -> [f32; 4] {
    let [r, g, b, _] = linear_rgba(color);
    [r * intensity, g * intensity, b * intensity, 0.0]
}

/// The camera `scene` is drawn from: its active camera, else the first in the tree, else a default.
pub(crate) fn camera_for(scene: &Scene) -> (Mat4, Projection) {
    let mut cameras = Vec::new();
    find_cameras(&scene.root(), Mat4::IDENTITY, &mut cameras);
    pick_camera(scene.inner().active_camera(), &cameras)
}

fn find_cameras(node: &Node, parent: Mat4, out: &mut Vec<(Node, Mat4, Projection)>) {
    if !node.visible() {
        return;
    }
    let world = parent * node.transform().to_matrix();
    if let Some(projection) = node.camera() {
        out.push((node.clone(), world, projection));
    }
    for child in node.children() {
        find_cameras(&child, world, out);
    }
}

pub(super) fn pick_camera(
    active: Option<Node>,
    cameras: &[(Node, Mat4, Projection)],
) -> (Mat4, Projection) {
    cameras
        .iter()
        .find(|(node, _, _)| Some(node) == active.as_ref())
        .or(cameras.first())
        .map(|(_, world, projection)| (*world, *projection))
        .unwrap_or_else(default_camera)
}

fn default_camera() -> (Mat4, Projection) {
    let transform = Transform::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
    (
        transform.to_matrix(),
        Projection::Perspective {
            fov_y: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
        },
    )
}
