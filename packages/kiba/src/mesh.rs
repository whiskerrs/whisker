use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Weak};

use glam::Vec3;

/// One corner of a [`Mesh`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Position relative to the entity.
    pub position: [f32; 3],
    /// Unit vector out of the surface, relative to the entity.
    pub normal: [f32; 3],
    /// Linear RGB color.
    pub color: [f32; 3],
}

#[derive(Debug)]
pub(crate) struct MeshData {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
}

/// How the surface of a [`Mesh`] reflects light, used as a component next to it.
///
/// A mesh without one uses the default, a white non-metal of medium roughness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// Linear RGB color, multiplied by the vertex colors.
    pub base_color: Vec3,
    /// 0 for a non-metal and 1 for a metal.
    pub metallic: f32,
    /// 0 for a mirror finish and 1 for fully matte.
    pub roughness: f32,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: Vec3::ONE,
            metallic: 0.0,
            roughness: 0.5,
        }
    }
}

/// Geometry that makes an entity visible, used as a component next to a [`Transform`](crate::Transform).
///
/// Clones share one GPU copy, which is released after the last clone is dropped.
#[derive(Clone, Debug)]
pub struct Mesh(Arc<MeshData>);

impl Mesh {
    /// Creates a mesh from triangles, whose front faces wind counter-clockwise.
    ///
    /// # Panics
    ///
    /// Panics if an index is out of range, or the index count is not a multiple of three.
    pub fn new(vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        assert!(
            indices.len().is_multiple_of(3),
            "mesh indices must form triangles"
        );
        assert!(
            indices
                .iter()
                .all(|&index| (index as usize) < vertices.len()),
            "mesh index out of range"
        );
        Self(Arc::new(MeshData { vertices, indices }))
    }

    /// Creates a white cube centered on the origin.
    pub fn cube(size: f32) -> Self {
        const FACES: [(Vec3, Vec3, Vec3); 6] = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::NEG_X, Vec3::Z, Vec3::Y),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::NEG_Y, Vec3::X, Vec3::Z),
            (Vec3::Z, Vec3::X, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y, Vec3::X),
        ];
        let half = size / 2.0;
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (normal, u, v) in FACES {
            let first = vertices.len() as u32;
            for corner in [-u - v, u - v, u + v, v - u] {
                vertices.push(Vertex {
                    position: ((normal + corner) * half).to_array(),
                    normal: normal.to_array(),
                    color: [1.0; 3],
                });
            }
            indices.extend([0, 1, 2, 0, 2, 3].map(|offset| first + offset));
        }
        Self::new(vertices, indices)
    }

    /// Creates a white sphere centered on the origin.
    pub fn sphere(radius: f32) -> Self {
        const SEGMENTS: u32 = 48;
        const RINGS: u32 = 24;
        let mut vertices = Vec::with_capacity(((SEGMENTS + 1) * (RINGS + 1)) as usize);
        for ring in 0..=RINGS {
            let polar = PI * ring as f32 / RINGS as f32;
            for segment in 0..=SEGMENTS {
                let azimuth = TAU * segment as f32 / SEGMENTS as f32;
                let normal = Vec3::new(
                    polar.sin() * azimuth.cos(),
                    polar.cos(),
                    polar.sin() * azimuth.sin(),
                );
                vertices.push(Vertex {
                    position: (normal * radius).to_array(),
                    normal: normal.to_array(),
                    color: [1.0; 3],
                });
            }
        }
        let mut indices = Vec::new();
        for ring in 0..RINGS {
            for segment in 0..SEGMENTS {
                let upper = ring * (SEGMENTS + 1) + segment;
                let lower = upper + SEGMENTS + 1;
                if ring > 0 {
                    indices.extend([upper, upper + 1, lower]);
                }
                if ring < RINGS - 1 {
                    indices.extend([upper + 1, lower + 1, lower]);
                }
            }
        }
        Self::new(vertices, indices)
    }

    /// Creates a triangle in the XY plane, facing +Z.
    pub fn triangle() -> Self {
        Self::new(
            vec![
                Vertex {
                    position: [0.0, 0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [1.0, 0.25, 0.25],
                },
                Vertex {
                    position: [-0.6, -0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.25, 1.0, 0.25],
                },
                Vertex {
                    position: [0.6, -0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.25, 0.25, 1.0],
                },
            ],
            vec![0, 1, 2],
        )
    }

    pub(crate) fn data(&self) -> &MeshData {
        &self.0
    }

    pub(crate) fn downgrade(&self) -> Weak<MeshData> {
        Arc::downgrade(&self.0)
    }

    /// Identifies the shared data. A [`Weak`] from [`Self::downgrade`] keeps the address unique.
    pub(crate) fn key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cube_triangle_faces_outward() {
        let cube = Mesh::cube(2.0);
        let data = cube.data();
        assert_eq!(data.indices.len(), 36);
        for triangle in data.indices.chunks(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(data.vertices[triangle[i] as usize].position));
            let normal = (b - a).cross(c - a);
            assert!(
                normal.dot((a + b + c) / 3.0) > 0.0,
                "{triangle:?} faces inward"
            );
            let declared = Vec3::from(data.vertices[triangle[0] as usize].normal);
            assert_eq!(normal.normalize(), declared);
        }
    }

    #[test]
    fn every_sphere_triangle_faces_outward_and_normals_point_radially() {
        let sphere = Mesh::sphere(2.0);
        let data = sphere.data();
        for vertex in &data.vertices {
            let position = Vec3::from(vertex.position);
            assert!((position / 2.0).abs_diff_eq(Vec3::from(vertex.normal), 1e-5));
        }
        for triangle in data.indices.chunks(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(data.vertices[triangle[i] as usize].position));
            let normal = (b - a).cross(c - a);
            assert!(
                normal.dot((a + b + c) / 3.0) > 0.0,
                "{triangle:?} faces inward"
            );
        }
    }

    #[test]
    fn clones_share_a_key_and_new_meshes_do_not() {
        let cube = Mesh::cube(1.0);
        assert_eq!(cube.key(), cube.clone().key());
        assert_ne!(cube.key(), Mesh::cube(1.0).key());
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn an_out_of_range_index_is_rejected() {
        Mesh::new(Vec::new(), vec![0, 0, 0]);
    }
}
