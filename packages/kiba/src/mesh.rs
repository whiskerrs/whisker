use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Weak};

use std::hash::{Hash, Hasher};

use glam::{Vec3, Vec4};

use crate::{Shader, Texture};

const SKINNED_BOUNDS_MARGIN: f32 = 2.0;

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
    /// Texture coordinates, with (0, 0) at the top left of an image.
    pub uv: [f32; 2],
}

/// Which joints move one [`Vertex`], in a stream of its own so that most meshes do not pay for it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SkinVertex {
    pub(crate) joints: [u16; 4],
    pub(crate) weights: [f32; 4],
}

#[derive(Debug)]
pub(crate) struct MeshData {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) skin: Option<Vec<SkinVertex>>,
    pub(crate) indices: Vec<u32>,
    pub(crate) bounds_center: Vec3,
    pub(crate) bounds_radius: f32,
}

/// How the surface of a [`Mesh`] reflects light, used as a component next to it.
///
/// A mesh without one uses the default, a white non-metal of roughness 0.5.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Linear RGB color, multiplied by the vertex colors.
    pub base_color: Vec3,
    /// 0 for a non-metal and 1 for a metal.
    pub metallic: f32,
    /// 0 for a mirror finish and 1 for fully matte.
    pub roughness: f32,
    /// Linear RGB light the surface gives off by itself, in the scale of [`Sun::color`](crate::Sun::color).
    /// It lights nothing else, so pair a lamp's glow with a [`PointLight`](crate::PointLight).
    pub emissive: Vec3,
    /// 1 for solid and 0 for invisible, multiplied by the base color texture's alpha. Only
    /// `alpha_mode` decides whether it has an effect.
    pub opacity: f32,
    pub alpha_mode: AlphaMode,
    /// Draws back faces too, lit as if they faced the viewer, for leaves, paper, and cloth.
    pub double_sided: bool,
    /// 0 to 1, the share of sunlight a thin surface lets through to its far side, for leaves, paper,
    /// and cloth.
    pub transmission: f32,
    /// sRGB color, multiplied by `base_color`, with opacity in alpha.
    pub base_color_texture: Option<Texture>,
    /// Roughness in green and metallic in blue, multiplied by their factors.
    pub metallic_roughness_texture: Option<Texture>,
    /// Surface direction in glTF's tangent space: +X along U, +Y against V, and +Z out.
    pub normal_texture: Option<Texture>,
    /// sRGB color, multiplied by `emissive`.
    pub emissive_texture: Option<Texture>,
    /// WGSL that moves the vertices and colors the surface in place of Kiba's own.
    pub shader: Option<Shader>,
    /// Values `shader` reads with `parameter(index)`. Entities whose parameters differ draw
    /// separately, so give many entities the same values where you can. Changes show the next frame.
    pub parameters: [Vec4; 8],
}

/// How a [`Material`]'s opacity shows what is behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AlphaMode {
    /// Opacity is ignored.
    #[default]
    Opaque,
    /// Fully solid where opacity reaches `cutoff` and absent elsewhere, for leaves and fences. It
    /// casts shadows with its holes and sorts like an opaque surface.
    Mask { cutoff: f32 },
    /// Blended over what is behind, for glass and water. Reflections stay at full strength, it casts
    /// no shadow, and surfaces are sorted back to front by their entity's position, so one that
    /// crosses another can draw in the wrong order.
    Blend,
}

impl Material {
    pub(crate) fn texture_keys(&self) -> [usize; 4] {
        [
            &self.base_color_texture,
            &self.metallic_roughness_texture,
            &self.normal_texture,
            &self.emissive_texture,
        ]
        .map(|texture| texture.as_ref().map_or(0, Texture::key))
    }

    /// What a batch must share beyond its textures, because it selects the pipeline.
    pub(crate) fn pipeline_key(&self) -> (u8, bool) {
        let mode = match self.alpha_mode {
            AlphaMode::Opaque => 0,
            AlphaMode::Mask { .. } => 1,
            AlphaMode::Blend => 2,
        };
        (mode, self.double_sided)
    }

    /// Zero without a shader, and otherwise the same only for the same shader and parameters.
    pub(crate) fn shader_key(&self) -> u64 {
        let Some(shader) = &self.shader else {
            return 0;
        };
        let mut hasher = rustc_hash::FxHasher::default();
        shader.key().hash(&mut hasher);
        for parameter in self.parameters {
            parameter.to_array().map(f32::to_bits).hash(&mut hasher);
        }
        hasher.finish().max(1)
    }
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: Vec3::ONE,
            metallic: 0.0,
            roughness: 0.5,
            emissive: Vec3::ZERO,
            opacity: 1.0,
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
            transmission: 0.0,
            base_color_texture: None,
            metallic_roughness_texture: None,
            normal_texture: None,
            emissive_texture: None,
            shader: None,
            parameters: [Vec4::ZERO; 8],
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
        let (min, max) = vertices.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), vertex| {
                let position = Vec3::from(vertex.position);
                (min.min(position), max.max(position))
            },
        );
        let bounds_center = if vertices.is_empty() {
            Vec3::ZERO
        } else {
            (min + max) / 2.0
        };
        let bounds_radius = vertices
            .iter()
            .map(|vertex| bounds_center.distance(vertex.position.into()))
            .fold(0.0, f32::max);
        Self(Arc::new(MeshData {
            vertices,
            skin: None,
            indices,
            bounds_center,
            bounds_radius,
        }))
    }

    /// Creates a mesh that bends with joints.
    pub(crate) fn skinned(vertices: Vec<Vertex>, indices: Vec<u32>, skin: Vec<SkinVertex>) -> Self {
        let rest = Self::new(vertices, indices);
        let data = Arc::try_unwrap(rest.0).expect("just created");
        Self(Arc::new(MeshData {
            skin: Some(skin),
            // Culling sees only the rest pose, and limbs reach beyond it.
            bounds_radius: data.bounds_radius * SKINNED_BOUNDS_MARGIN,
            ..data
        }))
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
            for (corner, uv) in [
                (-u - v, [0.0, 1.0]),
                (u - v, [1.0, 1.0]),
                (u + v, [1.0, 0.0]),
                (v - u, [0.0, 0.0]),
            ] {
                vertices.push(Vertex {
                    position: ((normal + corner) * half).to_array(),
                    normal: normal.to_array(),
                    color: [1.0; 3],
                    uv,
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
                    uv: [segment as f32 / SEGMENTS as f32, ring as f32 / RINGS as f32],
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

    /// Creates a white square with sides of `size` in the XZ plane, centered on the origin and facing
    /// +Y, with one image across it from uv (0, 0) at its -X, -Z corner.
    pub fn plane(size: f32) -> Self {
        let half = size / 2.0;
        let corner = |x: f32, z: f32, uv| Vertex {
            position: [x * half, 0.0, z * half],
            normal: [0.0, 1.0, 0.0],
            color: [1.0; 3],
            uv,
        };
        Self::new(
            vec![
                corner(-1.0, 1.0, [0.0, 1.0]),
                corner(1.0, 1.0, [1.0, 1.0]),
                corner(1.0, -1.0, [1.0, 0.0]),
                corner(-1.0, -1.0, [0.0, 0.0]),
            ],
            vec![0, 1, 2, 0, 2, 3],
        )
    }

    /// Creates a triangle in the XY plane, facing +Z.
    pub fn triangle() -> Self {
        Self::new(
            vec![
                Vertex {
                    position: [0.0, 0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [1.0, 0.25, 0.25],
                    uv: [0.5, 0.0],
                },
                Vertex {
                    position: [-0.6, -0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.25, 1.0, 0.25],
                    uv: [0.0, 1.0],
                },
                Vertex {
                    position: [0.6, -0.6, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    color: [0.25, 0.25, 1.0],
                    uv: [1.0, 1.0],
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
