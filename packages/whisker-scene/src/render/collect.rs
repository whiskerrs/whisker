//! Gathering what a frame draws from the node tree.

use glam::{Mat4, Vec3};
use std::rc::Rc;

use crate::core::{
    Deform, Geometry, Light, Material, Node, Projection, Scene, Transform, linear_rgba,
};

/// One mesh to draw.
pub(crate) struct Draw {
    pub(crate) model: Mat4,
    pub(crate) geometry: Geometry,
    pub(crate) material: Material,
    pub(crate) deform: Option<Rc<Deform>>,
}

/// What one frame draws, gathered from the node tree.
#[derive(Default)]
pub(crate) struct Collected {
    pub(crate) draws: Vec<Draw>,
    pub(crate) directional: Vec<(Vec3, [f32; 4])>,
    pub(crate) point: Vec<(Vec3, f32, [f32; 4])>,
    pub(crate) cameras: Vec<(Node, Mat4, Projection)>,
}

pub(crate) fn collect(root: &Node) -> Collected {
    let mut collected = Collected::default();
    visit(root, Mat4::IDENTITY, &mut collected);
    collected
}

fn visit(node: &Node, parent: Mat4, out: &mut Collected) {
    if !node.visible() {
        return;
    }
    let world = parent * node.transform().to_matrix();
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
        });
    }
    match node.light() {
        Some(Light::Directional { color, intensity }) => {
            let toward = -world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::Y);
            out.directional.push((toward, radiance(color, intensity)));
        }
        Some(Light::Point {
            color,
            intensity,
            range,
        }) => {
            out.point.push((
                world.transform_point3(Vec3::ZERO),
                range,
                radiance(color, intensity),
            ));
        }
        None => {}
    }
    if let Some(projection) = node.camera() {
        out.cameras.push((node.clone(), world, projection));
    }
    for child in node.children() {
        visit(&child, world, out);
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
