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
    pub(crate) gpu: OnceCell<crate::render::GpuMesh>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 3],
    pub(crate) normal: [f32; 3],
    pub(crate) uv: [f32; 2],
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
                vertices.push(Vertex {
                    position: position.to_array(),
                    normal: normal.to_array(),
                    uv: [(su + 1.0) / 2.0, (1.0 - sv) / 2.0],
                });
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
                vertices.push(Vertex {
                    position: (normal * radius).to_array(),
                    normal: normal.to_array(),
                    uv: [u, v],
                });
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
        .map(|(x, z, u, v)| Vertex {
            position: [x, 0.0, z],
            normal: [0.0, 1.0, 0.0],
            uv: [u, v],
        })
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
            .map(|(i, (position, normal))| Vertex {
                position: position.to_array(),
                normal: normal.to_array(),
                uv: uvs.as_ref().map_or([0.0, 0.0], |uvs| uvs[i].to_array()),
            })
            .collect();
        Geometry::from_parts(vertices, indices)
    }

    fn from_parts(vertices: Vec<Vertex>, indices: Vec<u32>) -> Geometry {
        Geometry {
            data: Rc::new(GeometryData {
                vertices,
                indices,
                gpu: OnceCell::new(),
            }),
        }
    }

    pub(crate) fn data(&self) -> &GeometryData {
        &self.data
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
    #[should_panic(expected = "out of range")]
    fn from_vertices_rejects_bad_indices() {
        Geometry::from_vertices(vec![Vec3::ZERO; 3], vec![Vec3::Y; 3], None, vec![0, 1, 3]);
    }
}
