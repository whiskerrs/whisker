use std::cell::OnceCell;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use glam::{Vec2, Vec3};

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
}
