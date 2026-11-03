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

    /// Creates a cube centered on the origin, with a different color on each face.
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
            let color = ((normal + Vec3::ONE) / 2.0).to_array();
            for corner in [-u - v, u - v, u + v, v - u] {
                vertices.push(Vertex {
                    position: ((normal + corner) * half).to_array(),
                    normal: normal.to_array(),
                    color,
                });
            }
            indices.extend([0, 1, 2, 0, 2, 3].map(|offset| first + offset));
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
