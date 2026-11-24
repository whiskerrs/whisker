use std::cell::OnceCell;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use glam::{Quat, Vec2, Vec3};

/// Triangle mesh data, uploaded to the GPU once and shared by every node that uses it.
#[derive(Clone)]
pub struct Geometry {
    data: Rc<GeometryData>,
}

pub(crate) struct GeometryData {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
    /// Up to four joints per vertex and their weights, for skinning.
    pub(crate) joints: Option<Vec<([u16; 4], [f32; 4])>>,
    pub(crate) morph_targets: Vec<MorphTarget>,
    pub(crate) gpu: OnceCell<crate::render::GpuMesh>,
    pub(crate) bvh: OnceCell<crate::raycast::Bvh>,
    bounds: OnceCell<(Vec3, f32)>,
}

/// Per-vertex offsets that a morph weight blends in; empty when the target leaves that attribute.
#[derive(Clone, Default)]
pub(crate) struct MorphTarget {
    pub(crate) positions: Vec<Vec3>,
    pub(crate) normals: Vec<Vec3>,
    pub(crate) tangents: Vec<Vec3>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
    /// xyz along increasing u; w is ±1 so that `cross(normal, xyz) * w` runs along increasing v.
    pub(crate) tangent: [f32; 4],
    /// Linear RGBA multiplied with the material's base color.
    pub(crate) color: [f32; 4],
}

impl Vertex {
    pub(crate) fn new(position: Vec3, normal: Vec3, uv: Vec2) -> Vertex {
        Vertex {
            position: position.to_array(),
            normal: normal.to_array(),
            uv: uv.to_array(),
            tangent: [1.0, 0.0, 0.0, 1.0],
            color: [1.0; 4],
        }
    }
}

impl PartialEq for Geometry {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }
}

impl std::fmt::Debug for Geometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Geometry")
            .field("vertices", &self.data.vertices.len())
            .field("triangles", &(self.data.indices.len() / 3))
            .finish()
    }
}

impl Geometry {
    /// A box centered on the origin.
    pub fn cuboid(size: Vec3) -> Geometry {
        let h = size / 2.0;
        // Each face: normal, then the in-plane axes u and v with u × v = normal.
        let faces = [
            (Vec3::X, Vec3::NEG_Z, Vec3::Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        ];
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (normal, u, v) in faces {
            let base = vertices.len() as u32;
            for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let position = (normal + u * su + v * sv) * h;
                vertices.push(Vertex::new(
                    position,
                    normal,
                    Vec2::new((su + 1.0) / 2.0, (1.0 - sv) / 2.0),
                ));
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Geometry::from_parts(vertices, indices)
    }

    /// A UV sphere centered on the origin with `segments` slices around and half as many from
    /// pole to pole.
    pub fn sphere(radius: f32, segments: u32) -> Geometry {
        let around = segments.max(3);
        let rings = (segments / 2).max(2);
        let mut vertices = Vec::with_capacity(((around + 1) * (rings + 1)) as usize);
        for ring in 0..=rings {
            let v = ring as f32 / rings as f32;
            let polar = v * PI;
            for slice in 0..=around {
                let u = slice as f32 / around as f32;
                let azimuth = u * TAU;
                let normal = Vec3::new(
                    polar.sin() * azimuth.sin(),
                    polar.cos(),
                    polar.sin() * azimuth.cos(),
                );
                vertices.push(Vertex::new(normal * radius, normal, Vec2::new(u, v)));
            }
        }
        let mut indices = Vec::with_capacity((around * rings * 6) as usize);
        let row = around + 1;
        for ring in 0..rings {
            for slice in 0..around {
                let a = ring * row + slice;
                let b = a + row;
                indices.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
        Geometry::from_parts(vertices, indices)
    }

    /// A rectangle in the XZ plane facing +Y, centered on the origin.
    pub fn plane(size: Vec2) -> Geometry {
        let (hx, hz) = (size.x / 2.0, size.y / 2.0);
        let vertices = [
            (-hx, hz, 0.0, 1.0),
            (hx, hz, 1.0, 1.0),
            (hx, -hz, 1.0, 0.0),
            (-hx, -hz, 0.0, 0.0),
        ]
        .map(|(x, z, u, v)| Vertex::new(Vec3::new(x, 0.0, z), Vec3::Y, Vec2::new(u, v)))
        .to_vec();
        Geometry::from_parts(vertices, vec![0, 1, 2, 0, 2, 3])
    }

    /// A capped cylinder along Y, centered on the origin.
    pub fn cylinder(radius: f32, height: f32, segments: u32) -> Geometry {
        let around = segments.max(3);
        let h = height / 2.0;
        let mut mesh = MeshBuilder::default();
        mesh.lathe(
            &[
                ProfilePoint::new(radius, h, Vec2::X, 0.0),
                ProfilePoint::new(radius, -h, Vec2::X, 1.0),
            ],
            around,
        );
        mesh.cap(h, 0.0, radius, true, around);
        mesh.cap(-h, 0.0, radius, false, around);
        mesh.build()
    }

    /// A cone with its base at `y = -height / 2` and its apex at `y = height / 2`, capped at the
    /// base.
    pub fn cone(radius: f32, height: f32, segments: u32) -> Geometry {
        let around = segments.max(3);
        let h = height / 2.0;
        let slope = Vec2::new(height, radius).normalize_or(Vec2::X);
        let mut mesh = MeshBuilder::default();
        mesh.lathe(
            &[
                ProfilePoint::new(0.0, h, slope, 0.0),
                ProfilePoint::new(radius, -h, slope, 1.0),
            ],
            around,
        );
        mesh.cap(-h, 0.0, radius, false, around);
        mesh.build()
    }

    /// A cylinder with hemispherical ends along Y, centered on the origin; `height` includes the
    /// ends.
    pub fn capsule(radius: f32, height: f32, segments: u32) -> Geometry {
        let around = segments.max(3);
        let rings = (segments / 4).max(2);
        let total = height.max(radius * 2.0);
        let half_body = total / 2.0 - radius;
        let mut profile = Vec::with_capacity(2 * (rings as usize + 1));
        for (center, from) in [(half_body, 0.0), (-half_body, PI / 2.0)] {
            for ring in 0..=rings {
                let polar = from + ring as f32 / rings as f32 * PI / 2.0;
                let normal = Vec2::new(polar.sin(), polar.cos());
                let y = center + radius * normal.y;
                let v = if total > 0.0 {
                    (total / 2.0 - y) / total
                } else {
                    0.0
                };
                profile.push(ProfilePoint::new(radius * normal.x, y, normal, v));
            }
        }
        let mut mesh = MeshBuilder::default();
        mesh.lathe(&profile, around);
        mesh.build()
    }

    /// A ring-shaped tube lying in the XZ plane, centered on the origin; `radius` reaches the
    /// middle of the tube.
    pub fn torus(radius: f32, tube: f32, segments: u32, tube_segments: u32) -> Geometry {
        let around = segments.max(3);
        let sides = tube_segments.max(3);
        let profile: Vec<ProfilePoint> = (0..=sides)
            .map(|side| {
                let v = side as f32 / sides as f32;
                let normal = Vec2::from_angle(TAU * (1.0 - v));
                ProfilePoint::new(radius + tube * normal.x, tube * normal.y, normal, v)
            })
            .collect();
        let mut mesh = MeshBuilder::default();
        mesh.lathe(&profile, around);
        mesh.build()
    }

    /// A circle in the XZ plane facing +Y, centered on the origin.
    pub fn disc(radius: f32, segments: u32) -> Geometry {
        let mut mesh = MeshBuilder::default();
        mesh.cap(0.0, 0.0, radius, true, segments.max(3));
        mesh.build()
    }

    /// A flat ring between the `inner` and `outer` radii in the XZ plane facing +Y, centered on
    /// the origin.
    pub fn ring(inner: f32, outer: f32, segments: u32) -> Geometry {
        let mut mesh = MeshBuilder::default();
        mesh.cap(0.0, inner, outer, true, segments.max(3));
        mesh.build()
    }

    /// A polygon in the XZ plane, given as `Vec2::new(x, z)` corners, extruded along +Y from
    /// `y = 0` to `y = depth` and capped at both ends.
    ///
    /// The outline closes on its own and may be concave but must not cross itself. Corners are
    /// expected counter-clockwise as seen from +Y; a clockwise outline is reversed.
    ///
    /// # Panics
    ///
    /// If the outline has fewer than three points.
    pub fn extrude(outline: &[Vec2], depth: f32) -> Geometry {
        let mut corners = outline.to_vec();
        if corners.len() > 3 && corners.first() == corners.last() {
            corners.pop();
        }
        assert!(
            corners.len() >= 3,
            "extrude needs an outline of at least three points"
        );
        if area_from_above(&corners) < 0.0 {
            corners.reverse();
        }
        let (min, max) = corners.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(min, max), corner| (min.min(*corner), max.max(*corner)),
        );
        let extent = (max - min).max(Vec2::splat(f32::EPSILON));
        let triangles = triangulate(&corners);
        let mut mesh = MeshBuilder::default();
        for (y, normal) in [(depth, Vec3::Y), (0.0, Vec3::NEG_Y)] {
            let base = mesh.vertices.len() as u32;
            for corner in &corners {
                mesh.vertices.push(Vertex::new(
                    Vec3::new(corner.x, y, corner.y),
                    normal,
                    (*corner - min) / extent,
                ));
            }
            for &[a, b, c] in &triangles {
                let (b, c) = if normal.y > 0.0 { (b, c) } else { (c, b) };
                mesh.indices.extend([base + a, base + b, base + c]);
            }
        }
        let count = corners.len();
        let perimeter: f32 = (0..count)
            .map(|i| corners[i].distance(corners[(i + 1) % count]))
            .sum();
        let mut walked = 0.0;
        for i in 0..count {
            let (from, to) = (corners[i], corners[(i + 1) % count]);
            let u0 = walked / perimeter.max(f32::EPSILON);
            walked += from.distance(to);
            let u1 = walked / perimeter.max(f32::EPSILON);
            let (a, b) = (Vec3::new(from.x, 0.0, from.y), Vec3::new(to.x, 0.0, to.y));
            let normal = (b - a).cross(Vec3::Y).normalize_or_zero();
            let top = Vec3::Y * depth;
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend([
                Vertex::new(a, normal, Vec2::new(u0, 1.0)),
                Vertex::new(b, normal, Vec2::new(u1, 1.0)),
                Vertex::new(b + top, normal, Vec2::new(u1, 0.0)),
                Vertex::new(a + top, normal, Vec2::new(u0, 0.0)),
            ]);
            mesh.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh.build()
    }

    /// An open tube of `radius` along the polyline `path`, its cross-sections carried along the
    /// path without twisting.
    ///
    /// # Panics
    ///
    /// If `path` has fewer than two distinct points.
    pub fn tube(path: &[Vec3], radius: f32, sides: u32) -> Geometry {
        let mut points: Vec<Vec3> = Vec::with_capacity(path.len());
        for &point in path {
            if points
                .last()
                .is_none_or(|last| last.distance_squared(point) > 1e-12)
            {
                points.push(point);
            }
        }
        assert!(
            points.len() >= 2,
            "a tube needs a path of at least two distinct points"
        );
        let sides = sides.max(3);
        let last = points.len() - 1;
        let segment = |i: usize| (points[i + 1] - points[i]).normalize();
        let tangents: Vec<Vec3> = (0..=last)
            .map(|i| match i {
                0 => segment(0),
                i if i == last => segment(last - 1),
                i => (segment(i - 1) + segment(i))
                    .try_normalize()
                    .unwrap_or(segment(i)),
            })
            .collect();
        let mut walked = vec![0.0; points.len()];
        for i in 1..=last {
            walked[i] = walked[i - 1] + points[i].distance(points[i - 1]);
        }
        let mut normal = tangents[0].any_orthonormal_vector();
        let mut mesh = MeshBuilder::default();
        for (i, (&point, &tangent)) in points.iter().zip(&tangents).enumerate() {
            if i > 0 {
                let carried = Quat::from_rotation_arc(tangents[i - 1], tangent) * normal;
                normal = (carried - tangent * tangent.dot(carried))
                    .try_normalize()
                    .unwrap_or_else(|| tangent.any_orthonormal_vector());
            }
            let binormal = tangent.cross(normal);
            for side in 0..=sides {
                let u = side as f32 / sides as f32;
                let (sin, cos) = (u * TAU).sin_cos();
                let out = normal * cos + binormal * sin;
                mesh.vertices.push(Vertex::new(
                    point + out * radius,
                    out,
                    Vec2::new(u, walked[i] / walked[last]),
                ));
            }
        }
        let row = sides + 1;
        for ring in 0..last as u32 {
            for side in 0..sides {
                let a = ring * row + side;
                let b = a + row;
                mesh.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
            }
        }
        mesh.build()
    }

    /// Triangles listed counter-clockwise when seen from the front.
    ///
    /// # Panics
    ///
    /// If `normals` or `uvs` differ in length from `positions`, the index count is not a multiple
    /// of three, or an index is out of range.
    pub fn from_vertices(
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Option<Vec<Vec2>>,
        indices: Vec<u32>,
    ) -> Geometry {
        assert_eq!(normals.len(), positions.len(), "one normal per position");
        if let Some(uvs) = &uvs {
            assert_eq!(uvs.len(), positions.len(), "one uv per position");
        }
        assert_eq!(indices.len() % 3, 0, "indices must form whole triangles");
        assert!(
            indices
                .iter()
                .all(|&index| (index as usize) < positions.len()),
            "an index is out of range"
        );
        let vertices = positions
            .iter()
            .zip(&normals)
            .enumerate()
            .map(|(i, (position, normal))| {
                Vertex::new(
                    *position,
                    *normal,
                    uvs.as_ref().map_or(Vec2::ZERO, |uvs| uvs[i]),
                )
            })
            .collect();
        Geometry::from_parts(vertices, indices)
    }

    /// Fills in tangents from the texture coordinates.
    pub(crate) fn from_parts(mut vertices: Vec<Vertex>, indices: Vec<u32>) -> Geometry {
        generate_tangents(&mut vertices, &indices);
        Geometry::from_parts_with_tangents(vertices, indices)
    }

    pub(crate) fn from_parts_with_tangents(vertices: Vec<Vertex>, indices: Vec<u32>) -> Geometry {
        Geometry::deformable(vertices, indices, None, Vec::new())
    }

    pub(crate) fn deformable(
        vertices: Vec<Vertex>,
        indices: Vec<u32>,
        joints: Option<Vec<([u16; 4], [f32; 4])>>,
        morph_targets: Vec<MorphTarget>,
    ) -> Geometry {
        Geometry {
            data: Rc::new(GeometryData {
                vertices,
                indices,
                joints,
                morph_targets,
                gpu: OnceCell::new(),
                bvh: OnceCell::new(),
                bounds: OnceCell::new(),
            }),
        }
    }

    pub(crate) fn data(&self) -> &GeometryData {
        &self.data
    }

    /// The center and radius of a sphere enclosing every rest-pose vertex.
    pub(crate) fn bounds(&self) -> (Vec3, f32) {
        *self.data.bounds.get_or_init(|| {
            let mut points = self.data.vertices.iter().map(|v| Vec3::from(v.position));
            let Some(first) = points.next() else {
                return (Vec3::ZERO, 0.0);
            };
            let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
            let center = (min + max) / 2.0;
            let radius = self
                .data
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position).distance_squared(center))
                .fold(0.0, f32::max)
                .sqrt();
            (center, radius)
        })
    }
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

/// A point of a profile swept around Y; `normal` is (outward from the axis, along Y).
struct ProfilePoint {
    radius: f32,
    y: f32,
    normal: Vec2,
    v: f32,
}

impl ProfilePoint {
    fn new(radius: f32, y: f32, normal: Vec2, v: f32) -> ProfilePoint {
        ProfilePoint {
            radius,
            y,
            normal,
            v,
        }
    }
}

fn around_y(u: f32) -> Vec3 {
    let (sin, cos) = (u * TAU).sin_cos();
    Vec3::new(sin, 0.0, cos)
}

impl MeshBuilder {
    /// Sweeps a profile, listed top to bottom along its outer side, around Y.
    fn lathe(&mut self, profile: &[ProfilePoint], around: u32) {
        let base = self.vertices.len() as u32;
        for point in profile {
            for slice in 0..=around {
                let u = slice as f32 / around as f32;
                let outward = around_y(u);
                let normal = (outward * point.normal.x + Vec3::Y * point.normal.y).normalize();
                self.vertices.push(Vertex::new(
                    outward * point.radius + Vec3::Y * point.y,
                    normal,
                    Vec2::new(u, point.v),
                ));
            }
        }
        let row = around + 1;
        for (ring, pair) in profile.windows(2).enumerate() {
            for slice in 0..around {
                let a = base + ring as u32 * row + slice;
                let b = a + row;
                if pair[0].radius != 0.0 {
                    self.indices.extend([a, b, a + 1]);
                }
                if pair[1].radius != 0.0 {
                    self.indices.extend([a + 1, b, b + 1]);
                }
            }
        }
    }

    /// A flat disc, or a ring when `inner` is positive, at height `y` facing up or down.
    fn cap(&mut self, y: f32, inner: f32, outer: f32, up: bool, around: u32) {
        let normal = if up { Vec3::Y } else { Vec3::NEG_Y };
        let uv = |position: Vec3| {
            Vec2::new(position.x, position.z) / (2.0 * outer).max(f32::EPSILON) + 0.5
        };
        let mut push = |position: Vec3| {
            self.vertices
                .push(Vertex::new(position, normal, uv(position)));
            self.vertices.len() as u32 - 1
        };
        let mut triangles = Vec::new();
        if inner > 0.0 {
            let rings: Vec<(u32, u32)> = (0..=around)
                .map(|slice| {
                    let outward = around_y(slice as f32 / around as f32);
                    let y = Vec3::Y * y;
                    (push(outward * inner + y), push(outward * outer + y))
                })
                .collect();
            for pair in rings.windows(2) {
                let [(in0, out0), (in1, out1)] = [pair[0], pair[1]];
                triangles.extend([[in0, out0, out1], [in0, out1, in1]]);
            }
        } else {
            let center = push(Vec3::Y * y);
            let rim: Vec<u32> = (0..=around)
                .map(|slice| push(around_y(slice as f32 / around as f32) * outer + Vec3::Y * y))
                .collect();
            for pair in rim.windows(2) {
                triangles.push([center, pair[0], pair[1]]);
            }
        }
        for [a, b, c] in triangles {
            let (b, c) = if up { (b, c) } else { (c, b) };
            self.indices.extend([a, b, c]);
        }
    }

    fn build(self) -> Geometry {
        Geometry::from_parts(self.vertices, self.indices)
    }
}

/// Twice the cross product of `a` and `b` as `Vec2::new(x, z)` vectors, positive when turning
/// counter-clockwise as seen from +Y.
fn cross_from_above(a: Vec2, b: Vec2) -> f32 {
    b.x * a.y - a.x * b.y
}

fn area_from_above(outline: &[Vec2]) -> f32 {
    let count = outline.len();
    (0..count)
        .map(|i| cross_from_above(outline[i], outline[(i + 1) % count]))
        .sum::<f32>()
        / 2.0
}

/// Ear clipping of a simple polygon wound counter-clockwise as seen from +Y.
fn triangulate(outline: &[Vec2]) -> Vec<[u32; 3]> {
    let turn = |a: Vec2, b: Vec2, c: Vec2| cross_from_above(b - a, c - a);
    let inside = |p: Vec2, a: Vec2, b: Vec2, c: Vec2| {
        turn(a, b, p) >= 0.0 && turn(b, c, p) >= 0.0 && turn(c, a, p) >= 0.0
    };
    let mut remaining: Vec<usize> = (0..outline.len()).collect();
    let mut triangles = Vec::with_capacity(outline.len() - 2);
    while remaining.len() > 3 {
        let count = remaining.len();
        let corners = |i: usize| {
            [(i + count - 1) % count, i, (i + 1) % count].map(|corner| remaining[corner])
        };
        let ear = (0..count)
            .find(|&i| {
                let [a, b, c] = corners(i);
                let [pa, pb, pc] = [a, b, c].map(|corner| outline[corner]);
                turn(pa, pb, pc) > 0.0
                    && remaining.iter().all(|&other| {
                        [a, b, c].contains(&other) || !inside(outline[other], pa, pb, pc)
                    })
            })
            // A degenerate outline has no clean ear left; clipping any corner still terminates.
            .unwrap_or(0);
        triangles.push(corners(ear).map(|corner| corner as u32));
        remaining.remove(ear);
    }
    triangles.push([remaining[0], remaining[1], remaining[2]].map(|corner| corner as u32));
    triangles
}

/// Per-vertex tangents averaged from each triangle's texture-coordinate gradient.
pub(crate) fn generate_tangents(vertices: &mut [Vertex], indices: &[u32]) {
    let mut along_u = vec![Vec3::ZERO; vertices.len()];
    let mut along_v = vec![Vec3::ZERO; vertices.len()];
    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|i| triangle[i] as usize);
        let [pa, pb, pc] = [a, b, c].map(|i| Vec3::from(vertices[i].position));
        let [ua, ub, uc] = [a, b, c].map(|i| Vec2::from(vertices[i].uv));
        let (e1, e2) = (pb - pa, pc - pa);
        let (d1, d2) = (ub - ua, uc - ua);
        let det = d1.x * d2.y - d2.x * d1.y;
        if det.abs() < 1e-12 {
            continue;
        }
        let r = 1.0 / det;
        let u = (e1 * d2.y - e2 * d1.y) * r;
        let v = (e2 * d1.x - e1 * d2.x) * r;
        for i in [a, b, c] {
            along_u[i] += u;
            along_v[i] += v;
        }
    }
    for (i, vertex) in vertices.iter_mut().enumerate() {
        let normal = Vec3::from(vertex.normal);
        let tangent = (along_u[i] - normal * normal.dot(along_u[i]))
            .try_normalize()
            .unwrap_or_else(|| normal.any_orthonormal_vector());
        let w = if normal.cross(tangent).dot(along_v[i]) < 0.0 {
            -1.0
        } else {
            1.0
        };
        vertex.tangent = tangent.extend(w).to_array();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_outward(geometry: &Geometry) {
        let data = geometry.data();
        for triangle in data.indices.chunks(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(data.vertices[triangle[i] as usize].position));
            let face = (b - a).cross(c - a);
            if face.length_squared() < 1e-12 {
                continue;
            }
            let normal = Vec3::from(data.vertices[triangle[0] as usize].normal);
            assert!(
                face.dot(normal) > 0.0,
                "triangle {triangle:?} winds against its normal"
            );
        }
    }

    #[test]
    fn primitives_wind_counter_clockwise_toward_their_normals() {
        assert_outward(&Geometry::cuboid(Vec3::new(1.0, 2.0, 3.0)));
        assert_outward(&Geometry::sphere(1.0, 16));
        assert_outward(&Geometry::plane(Vec2::new(2.0, 1.0)));
    }

    #[test]
    fn cuboid_spans_its_size() {
        let data_owner = Geometry::cuboid(Vec3::new(2.0, 4.0, 6.0));
        let data = data_owner.data();
        let max = data.vertices.iter().fold(Vec3::splat(f32::MIN), |max, v| {
            max.max(Vec3::from(v.position))
        });
        assert_eq!(max, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!((data.vertices.len(), data.indices.len()), (24, 36));
    }

    #[test]
    fn bounds_enclose_every_vertex_around_the_box_center() {
        let shifted = Geometry::from_vertices(
            vec![
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
                Vec3::new(3.0, 2.0, 0.0),
            ],
            vec![Vec3::Z; 3],
            None,
            vec![0, 1, 2],
        );
        let (center, radius) = shifted.bounds();
        assert_eq!(center, Vec3::new(2.0, 1.0, 0.0));
        assert!((radius - 2f32.sqrt()).abs() < 1e-6, "{radius}");
        assert_eq!(
            Geometry::from_vertices(vec![], vec![], None, vec![]).bounds(),
            (Vec3::ZERO, 0.0)
        );
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn from_vertices_rejects_bad_indices() {
        Geometry::from_vertices(vec![Vec3::ZERO; 3], vec![Vec3::Y; 3], None, vec![0, 1, 3]);
    }

    fn positions(geometry: &Geometry) -> Vec<Vec3> {
        geometry
            .data()
            .vertices
            .iter()
            .map(|vertex| Vec3::from(vertex.position))
            .collect()
    }

    fn bounds(geometry: &Geometry) -> (Vec3, Vec3) {
        positions(geometry).into_iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), p| (min.min(p), max.max(p)),
        )
    }

    /// Positive for a closed mesh whose triangles face outward.
    fn signed_volume(geometry: &Geometry) -> f32 {
        let positions = positions(geometry);
        geometry
            .data()
            .indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| positions[t[i] as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    fn assert_close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() <= tolerance * expected.abs().max(1.0),
            "{actual} is not close to {expected}"
        );
    }

    fn l_shape() -> Vec<Vec2> {
        [
            (0.0, 0.0),
            (2.0, 0.0),
            (2.0, -1.0),
            (1.0, -1.0),
            (1.0, -3.0),
            (0.0, -3.0),
        ]
        .map(|(x, z)| Vec2::new(x, z))
        .to_vec()
    }

    fn helix() -> Vec<Vec3> {
        (0..40)
            .map(|i| {
                let t = i as f32 * 0.3;
                Vec3::new(t.cos(), t * 0.1, t.sin())
            })
            .collect()
    }

    #[test]
    fn new_primitives_wind_counter_clockwise_toward_their_normals() {
        assert_outward(&Geometry::cylinder(0.5, 2.0, 24));
        assert_outward(&Geometry::cone(0.5, 2.0, 24));
        assert_outward(&Geometry::capsule(0.5, 2.0, 24));
        assert_outward(&Geometry::torus(1.0, 0.3, 32, 16));
        assert_outward(&Geometry::disc(1.0, 24));
        assert_outward(&Geometry::ring(0.5, 1.0, 24));
        assert_outward(&Geometry::extrude(&l_shape(), 0.5));
        assert_outward(&Geometry::tube(&helix(), 0.1, 12));
    }

    #[test]
    fn closed_primitives_enclose_their_volume() {
        let polygon = |n: f32, r: f32| n / 2.0 * r * r * (TAU / n).sin();
        assert_close(
            signed_volume(&Geometry::cylinder(0.5, 2.0, 64)),
            polygon(64.0, 0.5) * 2.0,
            1e-4,
        );
        assert_close(
            signed_volume(&Geometry::cone(0.5, 2.0, 64)),
            polygon(64.0, 0.5) * 2.0 / 3.0,
            1e-4,
        );
        let (r, h) = (0.5, 3.0);
        assert_close(
            signed_volume(&Geometry::capsule(r, h, 128)),
            PI * r * r * (h - 2.0 * r) + 4.0 / 3.0 * PI * r * r * r,
            0.01,
        );
        assert_close(
            signed_volume(&Geometry::torus(1.0, 0.25, 128, 64)),
            2.0 * PI * PI * 1.0 * 0.25 * 0.25,
            0.01,
        );
    }

    #[test]
    fn cylinder_cone_and_capsule_span_their_height() {
        for geometry in [
            Geometry::cylinder(0.5, 2.0, 16),
            Geometry::cone(0.5, 2.0, 16),
            Geometry::capsule(0.5, 2.0, 16),
        ] {
            let (min, max) = bounds(&geometry);
            assert_close(min.y, -1.0, 1e-5);
            assert_close(max.y, 1.0, 1e-5);
            assert_close(max.x, 0.5, 1e-5);
        }
        let cylinder = Geometry::cylinder(0.5, 2.0, 16);
        assert_eq!(cylinder.data().vertices.len(), 2 * 17 + 2 * 18);
        let (min, max) = bounds(&Geometry::capsule(0.5, 0.2, 16));
        assert_close(max.y - min.y, 1.0, 1e-5);
    }

    #[test]
    fn torus_reaches_radius_plus_tube() {
        let torus = Geometry::torus(1.0, 0.25, 32, 12);
        let (min, max) = bounds(&torus);
        assert_close(max.x, 1.25, 1e-5);
        assert_close(max.y, 0.25, 1e-3);
        assert_close(min.y, -0.25, 1e-3);
        let inner = positions(&torus)
            .iter()
            .map(|p| Vec2::new(p.x, p.z).length())
            .fold(f32::MAX, f32::min);
        assert_close(inner, 0.75, 1e-5);
        assert_eq!(torus.data().vertices.len(), 33 * 13);
    }

    #[test]
    fn disc_and_ring_face_up_between_their_radii() {
        for geometry in [Geometry::disc(1.0, 3), Geometry::ring(0.5, 1.0, 3)] {
            assert!(
                geometry
                    .data()
                    .vertices
                    .iter()
                    .all(|v| v.normal == [0.0, 1.0, 0.0])
            );
            assert!(positions(&geometry).iter().all(|p| p.y == 0.0));
        }
        let radii: Vec<f32> = positions(&Geometry::ring(0.5, 1.0, 8))
            .iter()
            .map(|p| p.length())
            .collect();
        assert!(
            radii
                .iter()
                .all(|r| (r - 0.5).abs() < 1e-5 || (r - 1.0).abs() < 1e-5)
        );
    }

    #[test]
    fn extruding_a_concave_outline_caps_its_exact_area() {
        let depth = 0.5;
        let area = 4.0;
        for outline in [l_shape(), l_shape().into_iter().rev().collect()] {
            let extruded = Geometry::extrude(&outline, depth);
            let data = extruded.data();
            let positions = positions(&extruded);
            let top: f32 = data
                .indices
                .chunks(3)
                .map(|t| [0, 1, 2].map(|i| t[i] as usize))
                .filter(|t| {
                    t.iter()
                        .all(|&i| data.vertices[i].normal == [0.0, 1.0, 0.0])
                })
                .map(|[a, b, c]| {
                    (positions[b] - positions[a])
                        .cross(positions[c] - positions[a])
                        .length()
                        / 2.0
                })
                .sum();
            assert_close(top, area, 1e-5);
            assert_close(signed_volume(&extruded), area * depth, 1e-5);
            let (min, max) = bounds(&extruded);
            assert_eq!((min.y, max.y), (0.0, depth));
        }
    }

    #[test]
    #[should_panic(expected = "at least three points")]
    fn extrude_rejects_a_degenerate_outline() {
        Geometry::extrude(&[Vec2::ZERO, Vec2::X], 1.0);
    }

    #[test]
    fn tube_normals_point_away_from_the_path_without_twisting() {
        let path = [Vec3::ZERO, Vec3::Z, Vec3::Z, Vec3::new(1.0, 0.0, 1.0)];
        let sides = 8;
        let tube = Geometry::tube(&path, 0.2, sides);
        let data = tube.data();
        assert_eq!(data.vertices.len(), 3 * (sides as usize + 1));
        let distinct = [path[0], path[1], path[3]];
        let first_offsets: Vec<Vec3> = data
            .vertices
            .chunks(sides as usize + 1)
            .zip(distinct)
            .map(|(ring, center)| {
                for vertex in ring {
                    let offset = Vec3::from(vertex.position) - center;
                    assert!(offset.dot(Vec3::from(vertex.normal)) > 0.0);
                }
                Vec3::from(ring[0].position) - center
            })
            .collect();
        let lift = first_offsets[0].y;
        for offset in &first_offsets {
            assert_close(offset.y, lift, 1e-4);
        }
    }

    #[test]
    #[should_panic(expected = "at least two distinct points")]
    fn tube_rejects_a_single_point() {
        Geometry::tube(&[Vec3::ONE, Vec3::ONE], 1.0, 8);
    }
}
