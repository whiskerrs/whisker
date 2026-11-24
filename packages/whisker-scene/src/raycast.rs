//! Picking nodes by casting rays through a scene.

use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::core::{Geometry, Node, Scene, Vertex};

/// A half-line from `origin` along `direction`, in the scene's world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    /// Unit length.
    pub direction: Vec3,
}

impl Ray {
    /// # Panics
    ///
    /// If `direction` has no length.
    pub fn new(origin: Vec3, direction: Vec3) -> Ray {
        Ray {
            origin,
            direction: direction.try_normalize().expect("a ray needs a direction"),
        }
    }

    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }
}

/// Where a ray met a mesh.
#[derive(Clone, Debug)]
pub struct SceneHit {
    /// The mesh node that was hit.
    pub node: Node,
    /// From the ray's origin, in world units.
    pub distance: f32,
    pub point: Vec3,
    /// The hit triangle's world-space normal, facing the ray.
    pub normal: Vec3,
    /// Texture coordinates at the hit point.
    pub uv: Vec2,
    /// Index of the hit triangle in the mesh.
    pub triangle: u32,
    /// Index of the hit instance, for a node drawn with [`Instances`](crate::Instances).
    pub instance: Option<u32>,
}

impl Scene {
    /// The ray through `point` of a view `size` wide and high, both in the same units, from the
    /// camera this scene is drawn from.
    pub fn ray_from_view(&self, point: Vec2, size: Vec2) -> Option<Ray> {
        if size.x <= 0.0 || size.y <= 0.0 {
            return None;
        }
        let (camera, projection) = crate::render::camera_for(self);
        let view_proj = projection.to_matrix(size.x / size.y) * camera.inverse();
        let inverse = view_proj.inverse();
        let ndc = Vec2::new(point.x / size.x * 2.0 - 1.0, 1.0 - point.y / size.y * 2.0);
        let unproject = |depth: f32| {
            let p = inverse * Vec4::new(ndc.x, ndc.y, depth, 1.0);
            p.truncate() / p.w
        };
        let (near, far) = (unproject(0.0), unproject(1.0));
        let direction = (far - near).try_normalize()?;
        Some(Ray {
            origin: near,
            direction,
        })
    }

    /// Where `world` appears in a view `size` wide and high, measured from its top-left corner in
    /// the same units, as seen from the camera this scene is drawn from.
    ///
    /// The inverse of [`Scene::ray_from_view`]. Points outside the view's edges are still
    /// returned; `None` means the point is behind the camera or outside its near and far range.
    pub fn view_point(&self, world: Vec3, size: Vec2) -> Option<Vec2> {
        if size.x <= 0.0 || size.y <= 0.0 {
            return None;
        }
        let (camera, projection) = crate::render::camera_for(self);
        project(
            projection.to_matrix(size.x / size.y) * camera.inverse(),
            world,
            size,
        )
    }

    /// Every visible mesh the ray meets, nearest first.
    pub fn raycast(&self, ray: Ray) -> Vec<SceneHit> {
        let mut hits = Vec::new();
        let eye = crate::render::camera_for(self)
            .0
            .transform_point3(Vec3::ZERO);
        visit(&self.root(), Mat4::IDENTITY, eye, &ray, &mut hits);
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        // A ray through an edge or corner meets every triangle sharing it.
        hits.dedup_by(|b, a| {
            a.node == b.node && a.instance == b.instance && (b.distance - a.distance).abs() < 1e-5
        });
        hits
    }
}

pub(crate) fn project(view_proj: Mat4, world: Vec3, size: Vec2) -> Option<Vec2> {
    let clip = view_proj * world.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !(0.0..=1.0).contains(&ndc.z) || !ndc.is_finite() {
        return None;
    }
    Some(Vec2::new(
        (ndc.x + 1.0) / 2.0 * size.x,
        (1.0 - ndc.y) / 2.0 * size.y,
    ))
}

fn visit(node: &Node, parent: Mat4, eye: Vec3, ray: &Ray, hits: &mut Vec<SceneHit>) {
    if !node.visible() {
        return;
    }
    let world = parent * node.transform().to_matrix();
    if !crate::render::within_distance(node, world, eye) {
        return;
    }
    if let Some((geometry, _)) = node.mesh() {
        let instances = node.instances();
        let transforms = instances
            .as_ref()
            .map_or(&[Mat4::IDENTITY][..], |i| i.transforms());
        for (index, transform) in transforms.iter().enumerate() {
            let first = hits.len();
            hit_mesh(node, &geometry, world * *transform, ray, hits);
            if instances.is_some() {
                for hit in &mut hits[first..] {
                    hit.instance = Some(index as u32);
                }
            }
        }
    }
    for child in node.children() {
        visit(&child, world, eye, ray, hits);
    }
}

fn hit_mesh(node: &Node, geometry: &Geometry, world: Mat4, ray: &Ray, hits: &mut Vec<SceneHit>) {
    match node.deform() {
        Some(deform) => {
            deform.update(geometry);
            let space = if deform.is_skinned() {
                Mat4::IDENTITY
            } else {
                world
            };
            deform.with_vertices(|vertices| {
                let indices = &geometry.data().indices;
                let local = local_ray(ray, space);
                for (triangle, corners) in indices.chunks_exact(3).enumerate() {
                    if let Some(hit) = hit_triangle(&local, vertices, corners) {
                        push_hit(hits, node, ray, space, vertices, corners, triangle, hit);
                    }
                }
            });
        }
        None => {
            let local = local_ray(ray, world);
            let data = geometry.data();
            let bvh = data.bvh.get_or_init(|| Bvh::build(geometry));
            bvh.each_hit(&local, &data.vertices, &data.indices, |triangle, hit| {
                let corners = &data.indices[triangle * 3..triangle * 3 + 3];
                push_hit(
                    hits,
                    node,
                    ray,
                    world,
                    &data.vertices,
                    corners,
                    triangle,
                    hit,
                );
            });
        }
    }
}

/// The ray in a mesh's own space; its direction is left unnormalized so hit parameters stay
/// comparable across spaces.
fn local_ray(ray: &Ray, space: Mat4) -> (Vec3, Vec3) {
    let inverse = space.inverse();
    (
        inverse.transform_point3(ray.origin),
        inverse.transform_vector3(ray.direction),
    )
}

#[allow(clippy::too_many_arguments)]
fn push_hit(
    hits: &mut Vec<SceneHit>,
    node: &Node,
    ray: &Ray,
    space: Mat4,
    vertices: &[Vertex],
    corners: &[u32],
    triangle: usize,
    (t, u, v): (f32, f32, f32),
) {
    let [a, b, c] = [0, 1, 2].map(|i| &vertices[corners[i] as usize]);
    let [pa, pb, pc] = [a, b, c].map(|vertex| space.transform_point3(vertex.position.into()));
    let mut normal = (pb - pa).cross(pc - pa).normalize_or_zero();
    if normal.dot(ray.direction) > 0.0 {
        normal = -normal;
    }
    let w = 1.0 - u - v;
    let uv = Vec2::from(a.uv) * w + Vec2::from(b.uv) * u + Vec2::from(c.uv) * v;
    hits.push(SceneHit {
        node: node.clone(),
        distance: t,
        point: ray.at(t),
        normal,
        uv,
        triangle: triangle as u32,
        instance: None,
    });
}

/// Möller–Trumbore against both faces: the ray parameter and the barycentric weights of the second
/// and third corners.
fn hit_triangle(
    (origin, direction): &(Vec3, Vec3),
    vertices: &[Vertex],
    corners: &[u32],
) -> Option<(f32, f32, f32)> {
    let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(vertices[corners[i] as usize].position));
    let (e1, e2) = (b - a, c - a);
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = *origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = direction.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0).then_some((t, u, v))
}

/// A bounding volume hierarchy over a geometry's triangles, built the first time it is picked.
pub(crate) struct Bvh {
    nodes: Vec<BvhNode>,
    triangles: Vec<u32>,
}

struct BvhNode {
    min: Vec3,
    max: Vec3,
    /// Leaves: the range in `triangles`. Inner nodes: `start` is the second child; the first
    /// follows this node.
    start: u32,
    count: u32,
}

const LEAF_SIZE: usize = 4;

impl Bvh {
    pub(crate) fn build(geometry: &Geometry) -> Bvh {
        let data = geometry.data();
        let corners = |triangle: u32| {
            let base = triangle as usize * 3;
            [0, 1, 2].map(|i| Vec3::from(data.vertices[data.indices[base + i] as usize].position))
        };
        let count = data.indices.len() / 3;
        let mut triangles: Vec<u32> = (0..count as u32).collect();
        let centers: Vec<Vec3> = (0..count as u32)
            .map(|triangle| {
                let [a, b, c] = corners(triangle);
                (a + b + c) / 3.0
            })
            .collect();
        let mut nodes = Vec::new();
        build(&mut nodes, &mut triangles, 0, count, &centers, &corners);
        Bvh { nodes, triangles }
    }

    fn each_hit(
        &self,
        ray: &(Vec3, Vec3),
        vertices: &[Vertex],
        indices: &[u32],
        mut found: impl FnMut(usize, (f32, f32, f32)),
    ) {
        if self.nodes.is_empty() {
            return;
        }
        let inverse = ray.1.recip();
        let mut stack = vec![0usize];
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index];
            if !slab(ray.0, inverse, node.min, node.max) {
                continue;
            }
            if node.count > 0 {
                let range = node.start as usize..(node.start + node.count) as usize;
                for &triangle in &self.triangles[range] {
                    let triangle = triangle as usize;
                    let corners = &indices[triangle * 3..triangle * 3 + 3];
                    if let Some(hit) = hit_triangle(ray, vertices, corners) {
                        found(triangle, hit);
                    }
                }
            } else {
                stack.push(index + 1);
                stack.push(node.start as usize);
            }
        }
    }
}

fn build(
    nodes: &mut Vec<BvhNode>,
    triangles: &mut [u32],
    offset: usize,
    count: usize,
    centers: &[Vec3],
    corners: &impl Fn(u32) -> [Vec3; 3],
) {
    let span = &mut triangles[offset..offset + count];
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for &triangle in span.iter() {
        for corner in corners(triangle) {
            min = min.min(corner);
            max = max.max(corner);
        }
    }
    let index = nodes.len();
    nodes.push(BvhNode {
        min,
        max,
        start: offset as u32,
        count: count as u32,
    });
    if count <= LEAF_SIZE {
        return;
    }
    let extent = max - min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    let half = count / 2;
    span.select_nth_unstable_by(half, |a, b| {
        centers[*a as usize][axis].total_cmp(&centers[*b as usize][axis])
    });
    nodes[index].count = 0;
    build(nodes, triangles, offset, half, centers, corners);
    nodes[index].start = nodes.len() as u32;
    build(
        nodes,
        triangles,
        offset + half,
        count - half,
        centers,
        corners,
    );
}

fn slab(origin: Vec3, inverse: Vec3, min: Vec3, max: Vec3) -> bool {
    let a = (min - origin) * inverse;
    let b = (max - origin) * inverse;
    let near = a.min(b).max_element();
    let far = a.max(b).min_element();
    // NaN from a zero direction against a slab face is unordered and so kept.
    far.partial_cmp(&near.max(0.0)) != Some(std::cmp::Ordering::Less)
}

#[cfg(test)]
mod tests;
