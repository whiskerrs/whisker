use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, LazyLock, Weak};

use glam::{Mat4, UVec3, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::animation::MAX_JOINTS;
use crate::lights::{LightUniform, MAX_LIGHTS};
use crate::mesh::{MeshData, SkinVertex, Vertex};
use crate::particles::{ParticleInstance, ParticlePass};
use crate::post::{PostPasses, PostView, SizedTargets};
use crate::probes::{self, ProbeVolume};
use crate::quality::Quality;
use crate::reflection::{Reflection, ReflectionTextures, face_view_projections};
use crate::sky::{Fog, Sky, SkyRenderer};
use crate::texture::{TextureData, mip_chain};
use crate::{AlphaMode, DepthOfField, Material, Mesh, Sun, Texture};

const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.02,
    g: 0.02,
    b: 0.04,
    a: 1.0,
};
pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
// Reversed depth (1 at the near plane) keeps floating-point precision over open-world distances.
const DEPTH_CLEAR: f32 = 0.0;
const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::GreaterEqual;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Instance {
    // Plain arrays because `Mat4`'s SIMD alignment would leave padding that `Pod` forbids.
    transform: [[f32; 4]; 4],
    base_color: [f32; 3],
    metallic: f32,
    roughness: f32,
    emissive: [f32; 3],
    /// Opacity, the cutoff below which a masked surface is absent, and transmission.
    opacity: [f32; 3],
}

impl Instance {
    pub(crate) fn new(transform: Mat4, material: &Material) -> Self {
        let cutoff = match material.alpha_mode {
            AlphaMode::Mask { cutoff } => cutoff,
            AlphaMode::Opaque | AlphaMode::Blend => 0.0,
        };
        Self {
            transform: transform.to_cols_array_2d(),
            base_color: material.base_color.to_array(),
            metallic: material.metallic,
            roughness: material.roughness,
            emissive: material.emissive.to_array(),
            opacity: [material.opacity, cutoff, material.transmission],
        }
    }

    fn origin_point(&self) -> Vec3 {
        Vec3::from_slice(&self.transform[3][..3])
    }

    #[cfg(test)]
    pub(crate) fn origin(&self) -> [f32; 3] {
        let [x, y, z, _] = self.transform[3];
        [x, y, z]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Batch {
    /// Joint matrices for a skinned mesh.
    pub(crate) palette: Option<Arc<[Mat4]>>,
    pub(crate) mesh: Mesh,
    /// Supplies the textures, which every instance shares.
    pub(crate) material: Material,
    pub(crate) instances: Vec<Instance>,
}

/// Everything [`SceneRenderer::draw`] needs to draw one frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub(crate) view_projection: Mat4,
    pub(crate) camera_position: Vec3,
    pub(crate) exposure: f32,
    pub(crate) sun: Sun,
    pub(crate) sky: Option<Sky>,
    pub(crate) probes: Option<ProbeVolume>,
    pub(crate) shadows: Option<Shadows>,
    pub(crate) batches: Vec<Batch>,
    pub(crate) effects: Effects,
}

/// What a frame needs beyond its opaque surfaces.
#[derive(Clone, Debug, Default)]
pub(crate) struct Effects {
    pub(crate) quality: Quality,
    pub(crate) projection: Mat4,
    pub(crate) depth_of_field: Option<DepthOfField>,
    pub(crate) camera_right: Vec3,
    pub(crate) camera_up: Vec3,
    pub(crate) fog: Option<Fog>,
    pub(crate) particles: Vec<ParticleInstance>,
    /// Point and spot lights, at most [`MAX_LIGHTS`].
    pub(crate) lights: Vec<LightUniform>,
    pub(crate) reflection: Option<Reflection>,
}

const UPLOAD_BYTES_PER_FRAME: u64 = 8 * 1024 * 1024;
const JOINTS_BINDING_SIZE: u64 = (MAX_JOINTS * size_of::<Mat4>()) as u64;
pub(crate) const MAX_CASCADES: usize = 4;

#[derive(Clone, Debug)]
pub(crate) struct Shadows {
    pub(crate) cascades: Vec<Cascade>,
    pub(crate) map_size: u32,
    /// Widens the penumbra with distance from the caster, rather than filtering a fixed width.
    pub(crate) soft: bool,
    /// What each cascade draws, in the same order.
    pub(crate) casters: Vec<Vec<Batch>>,
}

/// One slice of the view's distance, with a shadow map layer of its own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    pub(crate) view_projection: Mat4,
    /// Distance from the camera where the next cascade takes over.
    pub(crate) end: f32,
    /// World units across one shadow map texel.
    pub(crate) texel: f32,
    /// World units from the map's depth of zero to one.
    pub(crate) depth_range: f32,
}

/// The fixed scene of Hosts that do not run a [`Game`](crate::Game) yet.
#[doc(hidden)]
pub fn placeholder_scene() -> Frame {
    static TRIANGLE: LazyLock<Mesh> = LazyLock::new(Mesh::triangle);
    Frame {
        view_projection: Mat4::IDENTITY,
        camera_position: Vec3::Z,
        exposure: 1.0,
        sun: Sun::UNLIT,
        sky: None,
        probes: None,
        shadows: None,
        batches: vec![Batch {
            palette: None,
            mesh: TRIANGLE.clone(),
            material: Material::default(),
            instances: vec![Instance::new(Mat4::IDENTITY, &Material::default())],
        }],
        effects: Effects::default(),
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    view_projection: Mat4,
    camera_position: Vec4,
    to_sun: Vec4,
    sun_color: Vec4,
    ambient: Vec4,
    cascades: [Mat4; MAX_CASCADES],
    /// Where each cascade ends.
    shadow_ends: Vec4,
    /// Each cascade's texel width and depth range, in world units.
    shadow_scales: [Vec4; MAX_CASCADES],
    /// One for soft shadows, then the tangent of the sun's angular radius.
    shadow_shape: Vec4,
    probe_min: Vec4,
    probe_scale: Vec4,
    shadow_params: Vec4,
    fog: Vec4,
    lights: [LightUniform; MAX_LIGHTS],
    /// The reflection probe's box, with W at one when there is one.
    reflection_min: Vec4,
    reflection_max: Vec4,
    /// Where the probe captured from, with W as the width over which it fades in at the box's edge.
    reflection_center: Vec4,
}

// The real sun's, about a quarter of a degree, so a penumbra grows a centimeter for every two meters.
const SUN_ANGULAR_RADIUS: f32 = 0.0047;

impl From<&Frame> for FrameUniform {
    fn from(frame: &Frame) -> Self {
        let cascade = |i: usize| frame.shadows.as_ref().and_then(|s| s.cascades.get(i));
        let probe_box = frame
            .probes
            .as_ref()
            .and_then(ProbeVolume::data)
            .map(|data| {
                let (min, size) = probes::bounds(data);
                (min, size, data.spacing)
            });
        Self {
            view_projection: frame.view_projection,
            // The spare component keeps the uniform to four vectors.
            camera_position: frame.camera_position.extend(frame.exposure),
            to_sun: (-frame.sun.direction).normalize_or_zero().extend(0.0),
            sun_color: frame.sun.color.extend(0.0),
            ambient: frame.sun.ambient.extend(0.0),
            cascades: std::array::from_fn(|i| {
                cascade(i).map_or(Mat4::IDENTITY, |c| c.view_projection)
            }),
            shadow_ends: Vec4::from_array(std::array::from_fn(|i| {
                cascade(i).map_or(0.0, |c| c.end)
            })),
            shadow_scales: std::array::from_fn(|i| {
                cascade(i).map_or(Vec4::ZERO, |c| Vec4::new(c.texel, c.depth_range, 0.0, 0.0))
            }),
            shadow_shape: Vec4::new(
                f32::from(u8::from(frame.shadows.as_ref().is_some_and(|s| s.soft))),
                SUN_ANGULAR_RADIUS.tan(),
                0.0,
                0.0,
            ),
            probe_min: probe_box
                .map_or(Vec4::ZERO, |(min, _, spacing)| min.extend(spacing.recip())),
            probe_scale: probe_box.map_or(Vec4::ZERO, |(_, size, _)| size.recip().extend(1.0)),
            shadow_params: Vec4::new(
                frame
                    .shadows
                    .as_ref()
                    .map_or(0.0, |s| s.cascades.len() as f32),
                f32::from(u8::from(frame.shadows.is_some())),
                f32::from(u8::from(frame.sky.is_some())),
                frame.effects.lights.len().min(MAX_LIGHTS) as f32,
            ),
            fog: frame
                .effects
                .fog
                .map_or(Vec4::ZERO, |fog| fog.color.extend(fog.density)),
            lights: std::array::from_fn(|i| {
                frame.effects.lights.get(i).copied().unwrap_or_default()
            }),
            reflection_min: frame
                .effects
                .reflection
                .as_ref()
                .map_or(Vec4::ZERO, |r| r.id.min().extend(1.0)),
            reflection_max: frame
                .effects
                .reflection
                .as_ref()
                .map_or(Vec4::ZERO, |r| r.id.max().extend(0.0)),
            reflection_center: frame.effects.reflection.as_ref().map_or(Vec4::ZERO, |r| {
                r.id.center
                    .extend((r.id.size.min_element() * REFLECTION_FADE).max(1e-3))
            }),
        }
    }
}

// The share of a probe box's shortest side over which its reflection fades in.
const REFLECTION_FADE: f32 = 0.1;

impl FrameUniform {
    /// What one capture layer of a reflection probe draws with: no exposure and no probe of its own.
    fn capture(&self, view_projection: Mat4, center: Vec3) -> Self {
        Self {
            view_projection,
            camera_position: center.extend(1.0),
            reflection_min: Vec4::ZERO,
            ..*self
        }
    }
}

struct GpuMesh {
    source: Weak<MeshData>,
    vertices: wgpu::Buffer,
    skin: Option<wgpu::Buffer>,
    indices: wgpu::Buffer,
    index_count: u32,
    bytes: u64,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, mesh: &Mesh) -> Self {
        let data = mesh.data();
        Self {
            source: mesh.downgrade(),
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kiba vertices"),
                contents: bytemuck::cast_slice(&data.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            skin: data.skin.as_ref().map(|skin| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("kiba skin"),
                    contents: bytemuck::cast_slice(skin),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kiba indices"),
                contents: bytemuck::cast_slice(&data.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: data.indices.len() as u32,
            bytes: (size_of_val(&*data.vertices)
                + size_of_val(&*data.indices)
                + data.skin.as_ref().map_or(0, |skin| size_of_val(&**skin)))
                as u64,
        }
    }
}

struct GpuTexture {
    source: Weak<TextureData>,
    view: wgpu::TextureView,
    bytes: u64,
}

impl GpuTexture {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, texture: &Texture, srgb: bool) -> Self {
        let limit = device.limits().max_texture_dimension_2d;
        let levels: Vec<_> = mip_chain(texture.data(), srgb)
            .into_iter()
            .skip_while(|level| level.width.max(level.height) > limit)
            .collect();
        Self {
            source: texture.downgrade(),
            bytes: levels.iter().map(|level| level.pixels.len() as u64).sum(),
            view: upload_mips(device, queue, srgb, &levels),
        }
    }
}

fn upload_mips(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    srgb: bool,
    levels: &[crate::texture::MipLevel],
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("kiba texture"),
        size: wgpu::Extent3d {
            width: levels[0].width,
            height: levels[0].height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        },
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip_level, level) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip_level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &level.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(level.width * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: level.width,
                height: level.height,
                depth_or_array_layers: 1,
            },
        );
    }
    texture.create_view(&Default::default())
}

fn solid_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    srgb: bool,
    pixel: [u8; 4],
) -> wgpu::TextureView {
    let level = crate::texture::MipLevel {
        width: 1,
        height: 1,
        pixels: pixel.to_vec(),
    };
    upload_mips(device, queue, srgb, &[level])
}

struct GpuMaterial {
    sources: Vec<Weak<TextureData>>,
    bindings: wgpu::BindGroup,
}

fn probes_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    [visibility, bounce]: &[wgpu::TextureView; 2],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("kiba probes"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(visibility),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(bounce),
            },
        ],
    })
}

/// Makes a 3D texture of one byte-per-channel texel per cell, in X, then Y, then Z order.
pub(crate) fn volume_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    dimensions: UVec3,
    texels: &[[u8; 4]],
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: dimensions.x,
        height: dimensions.y,
        depth_or_array_layers: dimensions.z,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("kiba probes"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(dimensions.x * 4),
            rows_per_image: Some(dimensions.y),
        },
        size,
    );
    texture.create_view(&Default::default())
}

/// One depth layer per cascade, and the uniform each one's pass draws with.
struct ShadowMaps {
    /// Texels per side, then cascades.
    shape: (u32, usize),
    array: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    uniforms: Vec<wgpu::Buffer>,
    bindings: Vec<wgpu::BindGroup>,
}

impl ShadowMaps {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: u32, count: usize) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kiba shadow map"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: count as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let uniforms: Vec<_> = (0..count)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("kiba shadow light"),
                    size: size_of::<Mat4>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Self {
            shape: (size, count),
            // A single layer would otherwise default to a 2D view, which the shader cannot bind.
            array: texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            layers: (0..count as u32)
                .map(|layer| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_array_layer: layer,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                })
                .collect(),
            bindings: uniforms
                .iter()
                .map(|uniform| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("kiba shadow"),
                        layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        }],
                    })
                })
                .collect(),
            uniforms,
        }
    }
}

struct FrameSources<'a> {
    uniform: &'a wgpu::Buffer,
    shadow_maps: &'a wgpu::TextureView,
    shadow_sampler: &'a wgpu::Sampler,
    sky: &'a SkyRenderer,
    sky_sampler: &'a wgpu::Sampler,
    reflection: &'a ReflectionTextures,
}

fn frame_bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sources: &FrameSources<'_>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("kiba frame"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: sources.uniform,
                    offset: 0,
                    size: wgpu::BufferSize::new(size_of::<FrameUniform>() as u64),
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(sources.shadow_maps),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sources.shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&sources.sky.radiance_view()),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&sources.sky.irradiance_view()),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(sources.sky_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&sources.reflection.cube_view()),
            },
        ],
    })
}

const MASKED: u8 = 1;
const BLENDED: u8 = 2;

/// Which variant of the mesh or shadow pipeline a batch draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
    skinned: bool,
    /// Opaque, [`MASKED`], or [`BLENDED`].
    mode: u8,
    double_sided: bool,
}

impl PipelineKey {
    const ALL: [Self; 12] = {
        let mut all = [Self {
            skinned: false,
            mode: 0,
            double_sided: false,
        }; 12];
        let mut i = 0;
        while i < 12 {
            all[i] = Self {
                skinned: i & 1 != 0,
                mode: (i / 4) as u8,
                double_sided: i & 2 != 0,
            };
            i += 1;
        }
        all
    };

    fn of(material: &Material, skinned: bool) -> Self {
        let (mode, double_sided) = material.pipeline_key();
        Self {
            skinned,
            mode,
            double_sided,
        }
    }
}

/// Encodes the scene into any render target, on a device owned by the caller.
pub struct SceneRenderer {
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    shadow_pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    post: PostPasses,
    particles: ParticlePass,
    frame_uniform: wgpu::Buffer,
    frame_stride: u64,
    frame_bindings: wgpu::BindGroup,
    reflection: ReflectionTextures,
    sky: SkyRenderer,
    probes_layout: wgpu::BindGroupLayout,
    probes_bindings: wgpu::BindGroup,
    probes_key: usize,
    joints_layout: wgpu::BindGroupLayout,
    joints: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    shadow_maps: ShadowMaps,
    light_layout: wgpu::BindGroupLayout,
    frame_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    sky_sampler: wgpu::Sampler,
    meshes: HashMap<usize, GpuMesh>,
    material_layout: wgpu::BindGroupLayout,
    material_sampler: wgpu::Sampler,
    default_views: [wgpu::TextureView; 4],
    textures: HashMap<(usize, bool), GpuTexture>,
    materials: HashMap<[usize; 4], GpuMaterial>,
    instances: Option<wgpu::Buffer>,
    targets: Option<SizedTargets>,
}

impl SceneRenderer {
    /// Prepares the pipeline for targets of `format`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4,
            5 => Float32x4,
            6 => Float32x4,
            7 => Float32x4,
            8 => Float32x3,
            9 => Float32,
            10 => Float32,
            13 => Float32x3,
            14 => Float32x3,
        ];
        let frame_visibility = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let fragment_entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let float_texture = |view_dimension| wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension,
            multisampled: false,
        };
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: frame_visibility,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(size_of::<FrameUniform>() as u64),
                    },
                    count: None,
                },
                fragment_entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                fragment_entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                fragment_entry(3, float_texture(wgpu::TextureViewDimension::Cube)),
                fragment_entry(4, float_texture(wgpu::TextureViewDimension::Cube)),
                fragment_entry(5, filtering),
                fragment_entry(6, float_texture(wgpu::TextureViewDimension::Cube)),
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba material"),
            entries: &[
                fragment_entry(0, float_texture(wgpu::TextureViewDimension::D2)),
                fragment_entry(1, float_texture(wgpu::TextureViewDimension::D2)),
                fragment_entry(2, float_texture(wgpu::TextureViewDimension::D2)),
                fragment_entry(3, filtering),
                fragment_entry(4, float_texture(wgpu::TextureViewDimension::D2)),
            ],
        });
        let vertex_uniform = |has_dynamic_offset, min_binding_size| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset,
                min_binding_size,
            },
            count: None,
        };
        let joints_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba joints"),
            entries: &[vertex_uniform(
                true,
                wgpu::BufferSize::new(JOINTS_BINDING_SIZE),
            )],
        });
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba shadow"),
            entries: &[vertex_uniform(false, None)],
        });
        let probes_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba probes"),
            entries: &[
                fragment_entry(0, float_texture(wgpu::TextureViewDimension::D3)),
                fragment_entry(1, float_texture(wgpu::TextureViewDimension::D3)),
            ],
        });
        let pipeline_layout = |label, layouts: &[&wgpu::BindGroupLayout]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                push_constant_ranges: &[],
            })
        };
        // Both pipelines take the same layout, so bindings carry over when switching between them.
        let mesh_layout = pipeline_layout(
            "kiba mesh",
            &[
                &frame_layout,
                &material_layout,
                &joints_layout,
                &probes_layout,
            ],
        );
        let shadow_layout = pipeline_layout("kiba shadow", &[&light_layout, &joints_layout]);
        let masked_shadow_layout = pipeline_layout(
            "kiba masked shadow",
            &[&light_layout, &joints_layout, &material_layout],
        );
        let skin_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<SkinVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![11 => Uint16x4, 12 => Float32x4],
        };
        let shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let vertex_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32x3,
                2 => Float32x3,
                3 => Float32x2,
            ],
        };
        let instance_buffer = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instance_attributes,
        };
        let premultiplied = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let plain_buffers = [vertex_buffer.clone(), instance_buffer.clone()];
        let skinned_buffers = [
            vertex_buffer.clone(),
            instance_buffer.clone(),
            skin_buffer.clone(),
        ];
        let mut pipelines = HashMap::new();
        for key @ PipelineKey {
            skinned,
            mode,
            double_sided,
        } in PipelineKey::ALL
        {
            let blended = mode == BLENDED;
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("kiba mesh"),
                layout: Some(&mesh_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if skinned { "vs_skinned" } else { "vs_main" }),
                    compilation_options: Default::default(),
                    buffers: if skinned {
                        &skinned_buffers
                    } else {
                        &plain_buffers
                    },
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(["fs_main", "fs_masked", "fs_blended"][usize::from(mode)]),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: blended.then_some(wgpu::BlendState {
                            color: premultiplied,
                            alpha: premultiplied,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: !blended,
                    depth_compare: DEPTH_COMPARE,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
            pipelines.insert(key, pipeline);
        }
        let shadow_shader = device.create_shader_module(wgpu::include_wgsl!("shadow.wgsl"));
        let shadow_vertex = wgpu::VertexBufferLayout {
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
            ..vertex_buffer.clone()
        };
        let mut shadow_pipelines = HashMap::new();
        for key @ PipelineKey {
            skinned,
            mode,
            double_sided,
        } in PipelineKey::ALL
        {
            if mode == BLENDED {
                continue;
            }
            let masked = mode == MASKED;
            let position_only = [shadow_vertex.clone(), instance_buffer.clone()];
            let position_and_skin = [
                shadow_vertex.clone(),
                instance_buffer.clone(),
                skin_buffer.clone(),
            ];
            let buffers: &[wgpu::VertexBufferLayout<'_>] = match (masked, skinned) {
                (false, false) => &position_only,
                (false, true) => &position_and_skin,
                (true, false) => &plain_buffers,
                (true, true) => &skinned_buffers,
            };
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("kiba shadow"),
                layout: Some(if masked {
                    &masked_shadow_layout
                } else {
                    &shadow_layout
                }),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some(match (masked, skinned) {
                        (false, false) => "vs_main",
                        (false, true) => "vs_skinned",
                        (true, false) => "vs_masked",
                        (true, true) => "vs_masked_skinned",
                    }),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: masked.then(|| wgpu::FragmentState {
                    module: &shadow_shader,
                    entry_point: Some("fs_masked"),
                    compilation_options: Default::default(),
                    targets: &[],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            });
            shadow_pipelines.insert(key, pipeline);
        }
        let shadow_maps = ShadowMaps::new(device, &light_layout, 1024, 2);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kiba shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let frame_stride = (size_of::<FrameUniform>() as u64).next_multiple_of(u64::from(
            device.limits().min_uniform_buffer_offset_alignment,
        ));
        // The frame, then one slot for each layer of a reflection capture.
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kiba frame"),
            size: frame_stride * 7,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let reflection = ReflectionTextures::new(device);
        let sky = SkyRenderer::new(device);
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kiba sky"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let frame_bindings = frame_bindings(
            device,
            &frame_layout,
            &FrameSources {
                uniform: &frame_uniform,
                shadow_maps: &shadow_maps.array,
                shadow_sampler: &shadow_sampler,
                sky: &sky,
                sky_sampler: &sky_sampler,
                reflection: &reflection,
            },
        );
        Self {
            frame_uniform,
            frame_stride,
            frame_bindings,
            reflection,
            sky,
            shadow_pipelines,
            shadow_maps,
            light_layout,
            frame_layout,
            shadow_sampler,
            sky_sampler,
            meshes: HashMap::new(),
            material_layout,
            probes_bindings: probes_binding(
                device,
                &probes_layout,
                &[[255; 4], [0; 4]].map(|texel| volume_view(device, queue, UVec3::ONE, &[texel])),
            ),
            probes_layout,
            probes_key: 0,
            joints_layout,
            joints: None,
            material_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("kiba material"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                anisotropy_clamp: 4,
                ..Default::default()
            }),
            default_views: [
                solid_view(device, queue, true, [255; 4]),
                solid_view(device, queue, false, [255; 4]),
                solid_view(device, queue, false, [128, 128, 255, 255]),
                solid_view(device, queue, true, [255; 4]),
            ],
            textures: HashMap::new(),
            materials: HashMap::new(),
            instances: None,
            targets: None,
            post: PostPasses::new(device, solid_view(device, queue, false, [255; 4]), format),
            particles: ParticlePass::new(device),
            pipelines,
        }
    }

    /// Whether the frame must carry what to draw into reflection probe `id`.
    pub(crate) fn needs_capture(&self, id: &crate::reflection::ReflectionId) -> bool {
        self.reflection.captured != Some(*id)
    }

    /// Bytes of meshes and textures held on the GPU.
    pub fn gpu_bytes(&self) -> u64 {
        let meshes = self.meshes.values().map(|mesh| mesh.bytes);
        let textures = self.textures.values().map(|texture| texture.bytes);
        meshes.chain(textures).sum()
    }

    /// Draws one frame into `target`, which measures `size` physical pixels.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        frame: &Frame,
    ) {
        let batches = &frame.batches;
        self.meshes.retain(|_, mesh| mesh.source.strong_count() > 0);
        let casters = frame
            .shadows
            .iter()
            .flat_map(|s| s.casters.iter().flatten());
        self.materials
            .retain(|_, material| material.sources.iter().all(|s| s.strong_count() > 0));
        self.textures
            .retain(|_, texture| texture.source.strong_count() > 0);
        // A streamed cell can bring far more than one frame can upload without a visible hitch, so
        // what does not fit waits a frame and draws nothing until then.
        let mut upload_left = UPLOAD_BYTES_PER_FRAME;
        for batch in batches.iter().chain(casters.clone()) {
            if upload_left > 0 && !self.meshes.contains_key(&batch.mesh.key()) {
                let mesh = GpuMesh::new(device, &batch.mesh);
                upload_left = upload_left.saturating_sub(mesh.bytes);
                self.meshes.insert(batch.mesh.key(), mesh);
            }
        }
        for batch in batches.iter().chain(casters.clone()) {
            self.prepare_material(device, queue, &batch.material, &mut upload_left);
        }
        let capture = frame
            .effects
            .reflection
            .as_ref()
            .filter(|r| self.reflection.captured != Some(r.id))
            .and_then(|r| r.faces.as_deref().map(|faces| (r.id, faces)));
        let face_batches = capture.iter().flat_map(|(_, faces)| faces.iter().flatten());
        for batch in face_batches.clone() {
            if upload_left > 0 && !self.meshes.contains_key(&batch.mesh.key()) {
                let mesh = GpuMesh::new(device, &batch.mesh);
                upload_left = upload_left.saturating_sub(mesh.bytes);
                self.meshes.insert(batch.mesh.key(), mesh);
            }
            self.prepare_material(device, queue, &batch.material, &mut upload_left);
        }
        let joint_offsets = self.upload_joints(device, queue, frame);
        if let Some(volume) = frame.probes.as_ref().filter(|v| v.key() != self.probes_key) {
            let data = volume.data().expect("extraction keeps only loaded volumes");
            let views = probes::upload(device, queue, data);
            self.probes_bindings = probes_binding(device, &self.probes_layout, &views);
            self.probes_key = volume.key();
        }
        let quality = frame.effects.quality;
        let scaled = |side: u32| ((side as f32 * quality.render_scale).round() as u32).max(1);
        let internal_size = (scaled(size.0), scaled(size.1));
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| !targets.matches(internal_size, &quality))
        {
            self.targets = Some(self.post.targets(device, internal_size, &quality));
        }
        let targets = self.targets.as_ref().expect("allocated above");
        let post_view = PostView {
            projection: frame.effects.projection,
            view_projection: frame.view_projection,
            to_sun: -frame.sun.direction.normalize_or(Vec3::NEG_Y),
            sun_color: frame.sun.color,
            depth_of_field: frame.effects.depth_of_field,
        };

        let face_batches = capture.iter().flat_map(|(_, faces)| faces.iter().flatten());
        let instance_data: Vec<Instance> = batches
            .iter()
            .chain(casters)
            .chain(face_batches)
            .flat_map(|batch| batch.instances.iter().copied())
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&instance_data);
        if self
            .instances
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
        {
            self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kiba instances"),
                size: (bytes.len() as u64).next_power_of_two().max(256),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let instances = self.instances.as_ref().expect("allocated above");
        queue.write_buffer(instances, 0, bytes);
        let uniform = FrameUniform::from(frame);
        queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&uniform));
        if let Some((id, _)) = capture {
            for (layer, view_projection) in face_view_projections(id.center).into_iter().enumerate()
            {
                queue.write_buffer(
                    &self.frame_uniform,
                    self.frame_stride * (layer as u64 + 1),
                    bytemuck::bytes_of(&uniform.capture(view_projection, id.center)),
                );
            }
        }

        if let Some(shadows) = &frame.shadows
            && (shadows.map_size, shadows.cascades.len()) != self.shadow_maps.shape
        {
            let shape = (shadows.map_size, shadows.cascades.len());
            self.shadow_maps = ShadowMaps::new(device, &self.light_layout, shape.0, shape.1);
            self.frame_bindings = frame_bindings(
                device,
                &self.frame_layout,
                &FrameSources {
                    uniform: &self.frame_uniform,
                    shadow_maps: &self.shadow_maps.array,
                    shadow_sampler: &self.shadow_sampler,
                    sky: &self.sky,
                    sky_sampler: &self.sky_sampler,
                    reflection: &self.reflection,
                },
            );
        }
        self.sky.prepare(device, queue, frame);
        self.particles.prepare(
            device,
            queue,
            frame.view_projection,
            (frame.effects.camera_right, frame.effects.camera_up),
            &frame.effects.particles,
        );

        let mut encoder = device.create_command_encoder(&Default::default());
        let mut next_instance: u64 = batches.iter().map(|b| b.instances.len() as u64).sum();
        if let Some(shadows) = &frame.shadows {
            for (cascade, slice) in shadows.cascades.iter().enumerate() {
                queue.write_buffer(
                    &self.shadow_maps.uniforms[cascade],
                    0,
                    bytemuck::bytes_of(&slice.view_projection),
                );
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("kiba shadow"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_maps.layers[cascade],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_bind_group(0, &self.shadow_maps.bindings[cascade], &[]);
                let stride = size_of::<Instance>() as u64;
                let mut first = next_instance;
                for batch in &shadows.casters[cascade] {
                    let count = batch.instances.len() as u64;
                    let range = first * stride..(first + count) * stride;
                    first += count;
                    let Some(mesh) = self.meshes.get(&batch.mesh.key()) else {
                        continue;
                    };
                    if count == 0 || mesh.index_count == 0 {
                        continue;
                    }
                    if PipelineKey::of(&batch.material, false).mode == MASKED {
                        let Some(material) = self.materials.get(&batch.material.texture_keys())
                        else {
                            continue;
                        };
                        pass.set_bind_group(2, &material.bindings, &[]);
                    }
                    self.bind_skin(
                        &mut pass,
                        batch,
                        mesh,
                        &joint_offsets,
                        1,
                        &self.shadow_pipelines,
                    );
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_vertex_buffer(1, instances.slice(range));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..count as u32);
                }
                next_instance = first;
            }
        }
        if let Some((id, faces)) = capture {
            let stride = size_of::<Instance>() as u64;
            for (layer, batches) in faces.iter().enumerate() {
                let (color, depth) = self.reflection.layer_targets(layer as u32);
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("kiba reflection capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                let offset = (self.frame_stride * (layer as u64 + 1)) as u32;
                pass.set_bind_group(0, &self.frame_bindings, &[offset]);
                pass.set_bind_group(3, &self.probes_bindings, &[]);
                for batch in batches {
                    let count = batch.instances.len() as u64;
                    let range = next_instance * stride..(next_instance + count) * stride;
                    next_instance += count;
                    self.draw_batch(
                        &mut pass,
                        batch,
                        instances.slice(range),
                        count,
                        &joint_offsets,
                    );
                }
            }
            self.reflection.finish(
                device,
                &mut encoder,
                &self.sky.radiance_view(),
                frame.sky.is_some(),
                frame.sun.ambient,
            );
            self.reflection.captured = Some(id);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kiba main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.hdr,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                        store: if post_view.reads_depth(&quality) {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.frame_bindings, &[0]);
            pass.set_bind_group(3, &self.probes_bindings, &[]);
            let stride = size_of::<Instance>() as u64;
            let mut first = 0;
            let mut blended = Vec::new();
            for batch in batches {
                let count = batch.instances.len() as u64;
                let range = first * stride..(first + count) * stride;
                first += count;
                if PipelineKey::of(&batch.material, false).mode == BLENDED {
                    for (index, instance) in batch.instances.iter().enumerate() {
                        let distance = instance
                            .origin_point()
                            .distance_squared(frame.camera_position);
                        blended.push((distance, batch, range.start / stride + index as u64));
                    }
                    continue;
                }
                self.draw_batch(
                    &mut pass,
                    batch,
                    instances.slice(range),
                    count,
                    &joint_offsets,
                );
            }
            if frame.sky.is_some() {
                self.sky.draw(&mut pass);
                pass.set_bind_group(0, &self.frame_bindings, &[0]);
            }
            // Blending needs what is behind drawn first, so these go one at a time from the farthest.
            blended.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, batch, at) in blended {
                let slice = instances.slice(at * stride..(at + 1) * stride);
                self.draw_batch(&mut pass, batch, slice, 1, &joint_offsets);
            }
            self.particles
                .draw(&mut pass, frame.effects.particles.len());
        }
        self.post.encode(
            &mut encoder,
            queue,
            targets,
            (target, size),
            &quality,
            &post_view,
        );
        queue.submit([encoder.finish()]);
    }

    fn draw_batch(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &Batch,
        instances: wgpu::BufferSlice<'_>,
        count: u64,
        joint_offsets: &HashMap<usize, u32>,
    ) {
        let (Some(mesh), Some(material)) = (
            self.meshes.get(&batch.mesh.key()),
            self.materials.get(&batch.material.texture_keys()),
        ) else {
            return;
        };
        // Slicing an empty range panics in wgpu.
        if count == 0 || mesh.index_count == 0 {
            return;
        }
        self.bind_skin(pass, batch, mesh, joint_offsets, 2, &self.pipelines);
        pass.set_bind_group(1, &material.bindings, &[]);
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        pass.set_vertex_buffer(1, instances);
        pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..count as u32);
    }

    /// Copies every batch's joint matrices into one buffer, returning each palette's offset.
    fn upload_joints(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> HashMap<usize, u32> {
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment) as usize;
        let casters = frame
            .shadows
            .iter()
            .flat_map(|s| s.casters.iter().flatten());
        let faces = frame
            .effects
            .reflection
            .iter()
            .flat_map(|r| r.faces.iter().flat_map(|faces| faces.iter().flatten()));
        let mut offsets = HashMap::new();
        let mut bytes = Vec::new();
        for palette in frame
            .batches
            .iter()
            .chain(casters)
            .chain(faces)
            .filter_map(|b| b.palette.as_ref())
        {
            offsets.entry(palette.as_ptr() as usize).or_insert_with(|| {
                bytes.resize(bytes.len().next_multiple_of(align), 0);
                let offset = bytes.len() as u32;
                bytes.extend_from_slice(bytemuck::cast_slice(palette));
                offset
            });
        }
        // The shader binds a full-size array at every offset, even for a short palette.
        let needed = bytes.len() as u64 + JOINTS_BINDING_SIZE;
        if self
            .joints
            .as_ref()
            .is_none_or(|(buffer, _)| buffer.size() < needed)
        {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kiba joints"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("kiba joints"),
                layout: &self.joints_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(JOINTS_BINDING_SIZE),
                    }),
                }],
            });
            self.joints = Some((buffer, bindings));
        }
        let (buffer, _) = self.joints.as_ref().expect("allocated above");
        queue.write_buffer(buffer, 0, &bytes);
        offsets
    }

    /// Selects the pipeline for `batch`, whose group `slot` always needs some joint binding.
    fn bind_skin(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        batch: &Batch,
        mesh: &GpuMesh,
        offsets: &HashMap<usize, u32>,
        slot: u32,
        pipelines: &HashMap<PipelineKey, wgpu::RenderPipeline>,
    ) {
        let (_, bindings) = self.joints.as_ref().expect("allocated each frame");
        let posed = batch.palette.as_ref().zip(mesh.skin.as_ref());
        let offset = posed.map_or(0, |(palette, _)| offsets[&(palette.as_ptr() as usize)]);
        pass.set_bind_group(slot, bindings, &[offset]);
        pass.set_pipeline(&pipelines[&PipelineKey::of(&batch.material, posed.is_some())]);
        if let Some((_, skin)) = posed {
            pass.set_vertex_buffer(2, skin.slice(..));
        }
    }

    fn prepare_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: &Material,
        upload_left: &mut u64,
    ) {
        let key = material.texture_keys();
        if self.materials.contains_key(&key) {
            return;
        }
        // Only colors are sRGB, and an image may fill several slots.
        let slots = [
            (&material.base_color_texture, true),
            (&material.metallic_roughness_texture, false),
            (&material.normal_texture, false),
            (&material.emissive_texture, true),
        ];
        let mut sources = Vec::new();
        for (texture, srgb) in slots {
            if let Some(texture) = texture {
                sources.push(texture.downgrade());
                if let Entry::Vacant(slot) = self.textures.entry((texture.key(), srgb)) {
                    if *upload_left == 0 {
                        return;
                    }
                    let uploaded = GpuTexture::new(device, queue, texture, srgb);
                    *upload_left = upload_left.saturating_sub(uploaded.bytes);
                    slot.insert(uploaded);
                }
            }
        }
        let views = [0, 1, 2, 3].map(|slot| {
            let (texture, srgb) = slots[slot];
            texture
                .as_ref()
                .map_or(&self.default_views[slot], |texture| {
                    &self.textures[&(texture.key(), srgb)].view
                })
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.material_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(views[3]),
                },
            ],
        });
        self.materials
            .insert(key, GpuMaterial { sources, bindings });
    }
}

/// Draws one frame per [`Renderer::render`] call into a Host-supplied surface.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    scene: SceneRenderer,
}

impl Renderer {
    /// Selects a GPU compatible with `surface` and prepares the pipeline.
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| error.to_string())?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .find(|format| !format.is_srgb())
            .or(capabilities.formats.first())
            .copied()
            .ok_or("surface reports no formats")?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 0,
            height: 0,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
        };
        let scene = SceneRenderer::new(&device, &queue, format);
        let info = adapter.get_info();
        log(&format!(
            "using {:?} on {} as {format:?}",
            info.backend, info.name
        ));
        Ok(Self {
            surface,
            device,
            queue,
            config,
            scene,
        })
    }

    pub(crate) fn needs_capture(&self, id: &crate::reflection::ReflectionId) -> bool {
        self.scene.needs_capture(id)
    }

    /// Bytes of meshes and textures held on the GPU.
    pub(crate) fn gpu_bytes(&self) -> u64 {
        self.scene.gpu_bytes()
    }

    /// Draws one frame into the surface.
    pub fn render(&mut self, width: u32, height: u32, frame: &Frame) {
        if width == 0 || height == 0 {
            self.config.width = 0;
            self.config.height = 0;
            return;
        }
        if (width, height) != (self.config.width, self.config.height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        match self.surface.get_current_texture() {
            Ok(texture) => self.present(texture, frame),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
            }
            Err(error) => log(&error.to_string()),
        }
    }

    fn present(&mut self, texture: wgpu::SurfaceTexture, frame: &Frame) {
        let view = texture.texture.create_view(&Default::default());
        let size = (self.config.width, self.config.height);
        self.scene
            .draw(&self.device, &self.queue, &view, size, frame);
        texture.present();
    }
}

#[cfg(target_os = "android")]
pub fn log(message: &str) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(
            priority: i32,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> i32;
    }
    const ANDROID_LOG_INFO: i32 = 4;
    let text = std::ffi::CString::new(message.replace('\0', "")).expect("NUL bytes were removed");
    // SAFETY: both pointers are NUL-terminated strings that outlive the call.
    unsafe { __android_log_write(ANDROID_LOG_INFO, c"kiba".as_ptr(), text.as_ptr()) };
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn log(message: &str) {
    eprintln!("kiba: {message}");
}

#[cfg(target_arch = "wasm32")]
pub fn log(message: &str) {
    web_sys::console::log_1(&format!("kiba: {message}").into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;
    use glam::Quat;

    const SIZE: u32 = 64;

    fn render_to_pixels(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        frame: &Frame,
    ) -> Vec<u8> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        SceneRenderer::new(device, queue, format).draw(device, queue, &view, (SIZE, SIZE), frame);

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT * SIZE),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |result| {
            result.expect("map readback buffer");
        });
        device
            .poll(wgpu::PollType::Wait)
            .expect("wait for readback");
        buffer.slice(..).get_mapped_range().to_vec()
    }

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        Some(pollster::block_on(adapter.request_device(&Default::default())).expect("device"))
    }

    fn center_pixel(pixels: &[u8]) -> [u8; 3] {
        let at =
            SIZE as usize / 2 * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize + SIZE as usize / 2 * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    }

    fn quad(normal: Vec3, color: [f32; 3], transform: Mat4, material: Material) -> Batch {
        let material_for_instance = material.clone();
        let vertex = |x: f32, y: f32| Vertex {
            position: [x, y, 0.0],
            normal: normal.to_array(),
            color,
            uv: [(x + 1.0) / 2.0, (1.0 - y) / 2.0],
        };
        Batch {
            palette: None,
            material,
            mesh: Mesh::new(
                vec![
                    vertex(-1.0, -1.0),
                    vertex(1.0, -1.0),
                    vertex(1.0, 1.0),
                    vertex(-1.0, 1.0),
                ],
                vec![0, 1, 2, 0, 2, 3],
            ),
            instances: vec![Instance::new(transform, &material_for_instance)],
        }
    }

    fn surface(material: Material) -> Batch {
        quad(Vec3::Z, [1.0; 3], Mat4::IDENTITY, material)
    }

    /// A surface in front of the far plane, where a sky would draw over it.
    fn near_surface(material: Material) -> Batch {
        quad(
            Vec3::Z,
            [1.0; 3],
            Mat4::from_translation(Vec3::Z * 0.5),
            material,
        )
    }

    fn solid_quad(color: [f32; 3], depth: f32) -> Batch {
        quad(
            Vec3::Z,
            color,
            Mat4::from_translation(Vec3::new(0.0, 0.0, depth)),
            Material::default(),
        )
    }

    fn frame(sun: Sun, batches: Vec<Batch>) -> Frame {
        Frame {
            view_projection: Mat4::IDENTITY,
            camera_position: Vec3::Z * 2.0,
            exposure: 1.0,
            sun,
            sky: None,
            probes: None,
            shadows: None,
            batches,
            effects: Effects::default(),
        }
    }

    fn center_of(device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> [u8; 3] {
        center_pixel(&render_to_pixels(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            frame,
        ))
    }

    /// A sun dim enough that highlights stay below the 8-bit clamp.
    fn dim_sun(direction: Vec3) -> Sun {
        Sun {
            direction,
            color: Vec3::splat(0.1),
            ambient: Vec3::ZERO,
            shadow_distance: 0.0,
        }
    }

    const DARK_SUN: Sun = Sun {
        ambient: Vec3::ZERO,
        ..Sun::UNLIT
    };

    const MATTE: Material = Material {
        base_color: Vec3::ONE,
        metallic: 0.0,
        roughness: 1.0,
        emissive: Vec3::ZERO,
        opacity: 1.0,
        alpha_mode: crate::AlphaMode::Opaque,
        double_sided: false,
        transmission: 0.0,
        base_color_texture: None,
        metallic_roughness_texture: None,
        normal_texture: None,
        emissive_texture: None,
    };

    #[test]
    fn a_matte_surface_dims_as_it_turns_from_the_sun() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let center = |direction| {
            center_of(
                &device,
                &queue,
                &frame(dim_sun(direction), vec![surface(MATTE)]),
            )[0]
        };

        assert!(center(Vec3::NEG_Z) > center(Vec3::new(1.0, 0.0, -1.0)));
        assert_eq!(center(Vec3::Z), 0);
    }

    #[test]
    fn smoother_surfaces_have_a_tighter_and_brighter_highlight() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let center = |roughness, direction| {
            let material = Material { roughness, ..MATTE };
            center_of(
                &device,
                &queue,
                &frame(dim_sun(direction), vec![surface(material)]),
            )[0]
        };
        let mirror_angle = Vec3::NEG_Z;
        let off_angle = Vec3::new(1.0, 0.0, -1.0);

        assert!(center(0.2, mirror_angle) > center(0.6, mirror_angle));
        assert!(center(0.2, off_angle) < center(0.6, off_angle));
    }

    #[test]
    fn only_metals_tint_their_highlight() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let center = |metallic| {
            let material = Material {
                base_color: Vec3::X,
                metallic,
                roughness: 0.2,
                ..Material::default()
            };
            center_of(
                &device,
                &queue,
                &frame(dim_sun(Vec3::NEG_Z), vec![surface(material)]),
            )
        };

        assert_eq!(center(1.0)[1], 0);
        assert!(center(0.0)[1] > 0);
    }

    #[test]
    fn ambient_light_reaches_non_metals_and_metals_facing_away_from_the_sun() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sun = Sun {
            direction: Vec3::Z,
            color: Vec3::ONE,
            ambient: Vec3::splat(0.25),
            shadow_distance: 0.0,
        };
        for metallic in [0.0, 1.0] {
            let material = Material { metallic, ..MATTE };
            let center = center_of(&device, &queue, &frame(sun, vec![surface(material)]));
            assert!(center[0] > 32, "metallic {metallic}: {center:?}");
        }
    }

    #[test]
    fn vertex_colors_and_the_sun_tint_the_base_color() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sun = Sun {
            color: Vec3::new(1.0, 1.0, 0.0),
            ..dim_sun(Vec3::NEG_Z)
        };
        let material = Material {
            base_color: Vec3::new(1.0, 0.0, 1.0),
            metallic: 1.0,
            roughness: 1.0,
            ..Material::default()
        };
        // Yellow vertices under a magenta base color and a yellow sun leave only red.
        let batch = quad(Vec3::Z, [1.0, 1.0, 0.0], Mat4::IDENTITY, material);

        let center = center_of(&device, &queue, &frame(sun, vec![batch]));

        assert!(center[0] > 0);
        assert_eq!(center[1..], [0, 0]);
    }

    #[test]
    fn normals_stay_perpendicular_under_uneven_scale() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sun = dim_sun(Vec3::NEG_X);
        // The inverse transpose of this scale turns the normal (1, 1, 0) toward (1, 1/3, 0).
        let stretched = quad(
            Vec3::new(1.0, 1.0, 0.0).normalize(),
            [1.0; 3],
            Mat4::from_scale(Vec3::new(1.0, 3.0, 1.0)),
            MATTE,
        );
        let corrected = quad(
            Vec3::new(1.0, 1.0 / 3.0, 0.0).normalize(),
            [1.0; 3],
            Mat4::IDENTITY,
            MATTE,
        );

        let actual = center_of(&device, &queue, &frame(sun, vec![stretched]));
        let expected = center_of(&device, &queue, &frame(sun, vec![corrected]));

        assert!(expected[0] > 0);
        assert!(actual[0].abs_diff(expected[0]) <= 1);
    }

    #[test]
    fn the_nearest_surface_wins_in_any_draw_order() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let near = solid_quad([1.0, 0.0, 0.0], 0.75);
        let far = solid_quad([0.0, 1.0, 0.0], 0.25);

        for batches in [
            vec![near.clone(), far.clone()],
            vec![far.clone(), near.clone()],
        ] {
            let [red, green, _] = center_of(&device, &queue, &frame(Sun::UNLIT, batches));
            assert!(red > green);
        }
    }

    #[test]
    fn back_faces_are_not_drawn() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let turned =
            Mat4::from_rotation_y(std::f32::consts::PI) * Mat4::from_scale(Vec3::splat(0.5));
        let batch = Batch {
            palette: None,
            mesh: Mesh::triangle(),
            material: Material::default(),
            instances: vec![Instance::new(turned, &Material::default())],
        };

        let center = center_of(&device, &queue, &frame(Sun::UNLIT, vec![batch]));

        assert!(center[0] < 64);
    }

    #[test]
    fn the_base_color_texture_is_sampled_at_the_uv_coordinates() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let image = Texture::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]);
        let center = |x: f32| {
            let material = Material {
                base_color_texture: Some(image.clone()),
                ..MATTE
            };
            let batch = quad(
                Vec3::Z,
                [1.0; 3],
                Mat4::from_translation(Vec3::new(x, 0.0, 0.0)),
                material,
            );
            center_of(&device, &queue, &frame(Sun::UNLIT, vec![batch]))
        };

        let left = center(0.5);
        assert!(left[0] > left[1], "{left:?}");
        let right = center(-0.5);
        assert!(right[1] > right[0], "{right:?}");
    }

    #[test]
    fn the_metallic_roughness_texture_scales_the_factors() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let center = |pixel: [u8; 4]| {
            let material = Material {
                base_color: Vec3::X,
                metallic: 1.0,
                roughness: 1.0,
                metallic_roughness_texture: Some(Texture::from_rgba8(1, 1, pixel.to_vec())),
                ..Material::default()
            };
            center_of(
                &device,
                &queue,
                &frame(
                    Sun {
                        color: Vec3::ONE,
                        ..dim_sun(Vec3::NEG_Z)
                    },
                    vec![surface(material)],
                ),
            )
        };

        assert_eq!(
            center([0, 255, 255, 255])[1],
            0,
            "blue should keep it metal"
        );
        assert!(
            center([0, 255, 0, 255])[1] > 0,
            "no blue should remove metal"
        );
        assert!(center([0, 64, 255, 255])[0] > center([0, 255, 255, 255])[0]);
    }

    #[test]
    fn the_normal_texture_follows_gltf_tangent_axes() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let lit = |pixel: [u8; 4], direction: Vec3| {
            let material = Material {
                normal_texture: Some(Texture::from_rgba8(1, 1, pixel.to_vec())),
                ..MATTE
            };
            center_of(
                &device,
                &queue,
                &frame(dim_sun(direction), vec![surface(material)]),
            )[0] > 0
        };

        // On this quad, U runs along +X and V along -Y, so tangent +Y is world +Y.
        let to_right = [255, 128, 128, 255];
        assert!(lit(to_right, Vec3::NEG_X));
        assert!(!lit(to_right, Vec3::X));
        let to_top = [128, 255, 128, 255];
        assert!(lit(to_top, Vec3::NEG_Y));
        assert!(!lit(to_top, Vec3::Y));
    }

    #[test]
    fn dropped_textures_release_their_gpu_copy() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut scene = SceneRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let material = Material {
            base_color_texture: Some(Texture::from_rgba8(1, 1, vec![255; 4])),
            ..Material::default()
        };
        let drawn = frame(Sun::UNLIT, vec![surface(material)]);

        scene.draw(&device, &queue, &view, (SIZE, SIZE), &drawn);
        assert_eq!(scene.textures.len(), 1);
        assert_eq!(scene.materials.len(), 1);

        drop(drawn);
        scene.draw(
            &device,
            &queue,
            &view,
            (SIZE, SIZE),
            &frame(Sun::UNLIT, Vec::new()),
        );
        assert!(scene.textures.is_empty());
        assert!(scene.materials.is_empty());
    }

    #[test]
    fn exposure_scales_the_scene_and_tone_mapping_keeps_highlights_below_white_clipping() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let at = |exposure, sun_strength: f32| {
            let mut drawn = frame(
                Sun {
                    color: Vec3::splat(sun_strength),
                    ..dim_sun(Vec3::NEG_Z)
                },
                vec![surface(MATTE)],
            );
            drawn.exposure = exposure;
            center_of(&device, &queue, &drawn)[0]
        };

        assert!(at(2.0, 0.2) > at(1.0, 0.2));
        assert!(
            at(1.0, 2.0) > at(1.0, 0.5),
            "bright light still has gradation"
        );
    }

    #[test]
    fn a_blocked_sun_darkens_the_ground_even_when_the_caster_is_off_screen() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let ground_brightness = |with_caster: bool| {
            let mut world = hecs::World::new();
            world.spawn((
                crate::Camera::default(),
                crate::Transform::looking_at(Vec3::Y, Vec3::ZERO, Vec3::Z),
            ));
            world.spawn((Sun {
                direction: Vec3::NEG_Y,
                ..Sun::default()
            },));
            world.spawn((crate::Transform::default(), Mesh::plane(20.0)));
            if with_caster {
                world.spawn((
                    crate::Transform {
                        translation: Vec3::Y * 5.0,
                        ..crate::Transform::default()
                    },
                    Mesh::cube(4.0),
                ));
            }
            let frame = crate::game::extract_frame(&world, SIZE, SIZE, 1.0, &Quality::default());
            center_of(&device, &queue, &frame)[0]
        };

        let lit = ground_brightness(false);
        let shadowed = ground_brightness(true);
        assert!(lit > shadowed + 32, "lit {lit}, shadowed {shadowed}");
    }

    #[test]
    fn a_masked_caster_lets_the_sun_through_its_holes() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let ground_brightness = |opacity: u8| {
            let mut world = hecs::World::new();
            world.spawn((
                crate::Camera::default(),
                crate::Transform::looking_at(Vec3::Y, Vec3::ZERO, Vec3::Z),
            ));
            world.spawn((Sun {
                direction: Vec3::NEG_Y,
                ..Sun::default()
            },));
            world.spawn((crate::Transform::default(), Mesh::plane(20.0)));
            world.spawn((
                crate::Transform::at(Vec3::Y * 5.0),
                Mesh::plane(8.0),
                Material {
                    alpha_mode: AlphaMode::Mask { cutoff: 0.5 },
                    double_sided: true,
                    base_color_texture: Some(Texture::from_rgba8(
                        1,
                        1,
                        vec![255, 255, 255, opacity],
                    )),
                    ..Material::default()
                },
            ));
            let frame = crate::game::extract_frame(&world, SIZE, SIZE, 1.0, &Quality::default());
            center_of(&device, &queue, &frame)[0]
        };

        let through_hole = ground_brightness(0);
        let under_leaf = ground_brightness(255);
        assert!(
            through_hole > under_leaf + 32,
            "{through_hole} {under_leaf}"
        );
    }

    #[test]
    fn emissive_surfaces_glow_without_any_light() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let glow = |emissive: Vec3| {
            let material = Material { emissive, ..MATTE };
            center_of(
                &device,
                &queue,
                &frame(DARK_SUN, vec![near_surface(material)]),
            )
        };
        assert_eq!(glow(Vec3::ZERO), [0, 0, 0]);
        let red = glow(Vec3::new(1.0, 0.0, 0.0));
        assert!(red[0] > 150 && red[1] == 0, "{red:?}");
    }

    #[test]
    fn opacity_hides_masked_surfaces_and_blends_blended_ones() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let over_red = |alpha_mode: AlphaMode, opacity: f32| {
            let material = Material {
                alpha_mode,
                opacity,
                ..Material::default()
            };
            let front = quad(
                Vec3::Z,
                [0.0, 0.0, 1.0],
                Mat4::from_translation(Vec3::Z * 0.6),
                material,
            );
            let frame = Frame {
                sky: Some(Sky::default()),
                ..frame(Sun::UNLIT, vec![front, solid_quad([1.0, 0.0, 0.0], 0.3)])
            };
            center_of(&device, &queue, &frame)
        };
        let mask = AlphaMode::Mask { cutoff: 0.5 };

        let hidden = over_red(mask, 0.3);
        let solid = over_red(mask, 0.8);
        let mixed = over_red(AlphaMode::Blend, 0.5);
        assert!(hidden[0] > 80 && hidden[2] < 30, "{hidden:?}");
        assert!(solid[2] > 80 && solid[0] < 30, "{solid:?}");
        assert!(mixed[0] > 40 && mixed[2] > 40, "{mixed:?}");
    }

    #[test]
    fn transmission_lets_sun_behind_a_surface_through() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let backlit = |transmission: f32| {
            let material = Material {
                transmission,
                ..MATTE
            };
            let batch = quad(Vec3::Z, [1.0; 3], Mat4::IDENTITY, material);
            center_of(&device, &queue, &frame(dim_sun(Vec3::Z), vec![batch]))[0]
        };
        let opaque = backlit(0.0);
        assert!(backlit(1.0) > opaque + 20, "{opaque}");
    }

    #[test]
    fn back_faces_draw_only_on_double_sided_materials() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let back = |double_sided: bool| {
            let material = Material {
                emissive: Vec3::ONE,
                double_sided,
                ..MATTE
            };
            let turned =
                Mat4::from_translation(Vec3::Z * 0.5) * Mat4::from_rotation_y(std::f32::consts::PI);
            let batch = quad(Vec3::Z, [1.0; 3], turned, material);
            center_of(&device, &queue, &frame(DARK_SUN, vec![batch]))[0]
        };
        let culled = back(false);
        assert!(back(true) > culled + 100, "{culled}");
    }

    #[test]
    fn a_sky_fills_the_background_and_lights_surfaces_by_its_color() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let red = Sky {
            zenith: Vec3::new(0.5, 0.0, 0.0),
            horizon: Vec3::new(0.5, 0.0, 0.0),
            ground: Vec3::new(0.5, 0.0, 0.0),
        };
        let shot = |batches, sky| {
            let frame = Frame {
                sky,
                ..frame(DARK_SUN, batches)
            };
            center_of(&device, &queue, &frame)
        };

        let background = shot(Vec::new(), Some(red));
        assert!(background[0] > 100 && background[1] == 0, "{background:?}");

        let matte = shot(vec![near_surface(MATTE)], Some(red));
        assert!(matte[0] > 100 && matte[1] == 0, "{matte:?}");
        let mirror = Material {
            metallic: 1.0,
            roughness: 0.1,
            ..MATTE
        };
        let reflecting = shot(vec![near_surface(mirror)], Some(red));
        assert!(reflecting[0] > 100 && reflecting[1] == 0, "{reflecting:?}");

        assert_eq!(shot(vec![near_surface(MATTE)], None)[0], 0);
    }

    #[test]
    fn a_metal_mirror_shows_the_part_of_the_sky_it_faces() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sky = Sky {
            zenith: Vec3::new(0.8, 0.0, 0.0),
            horizon: Vec3::new(0.8, 0.0, 0.0),
            ground: Vec3::new(0.0, 0.0, 0.8),
        };
        let mirror = Material {
            metallic: 1.0,
            roughness: 0.1,
            ..MATTE
        };
        let tilted = |angle: f32| {
            let mut batch = near_surface(mirror.clone());
            let placed = Mat4::from_translation(Vec3::Z * 0.5) * Mat4::from_rotation_x(angle);
            batch.instances = vec![Instance::new(placed, &mirror)];
            let frame = Frame {
                sky: Some(sky),
                ..frame(DARK_SUN, vec![batch])
            };
            center_of(&device, &queue, &frame)
        };

        // The camera looks down -Z at a surface facing +Z, so tilting its face up reflects the sky.
        let up = tilted(-0.6);
        let down = tilted(0.6);
        assert!(up[0] > up[2] && down[2] > down[0], "{up:?} {down:?}");
    }

    #[test]
    fn joints_move_the_vertices_they_weigh_on() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let corner = |x: f32, y: f32| Vertex {
            position: [x, y, 0.0],
            normal: [0.0, 0.0, 1.0],
            color: [1.0; 3],
            uv: [0.0; 2],
        };
        let mesh = Mesh::skinned(
            vec![
                corner(-0.2, -0.2),
                corner(0.2, -0.2),
                corner(0.2, 0.2),
                corner(-0.2, 0.2),
            ],
            vec![0, 1, 2, 0, 2, 3],
            vec![
                SkinVertex {
                    joints: [1, 0, 0, 0],
                    weights: [1.0, 0.0, 0.0, 0.0],
                };
                4
            ],
        );
        let shot = |shift: f32| {
            let palette: Arc<[Mat4]> =
                vec![Mat4::IDENTITY, Mat4::from_translation(Vec3::X * shift)].into();
            let batch = Batch {
                mesh: mesh.clone(),
                material: MATTE,
                palette: Some(palette),
                instances: vec![Instance::new(Mat4::IDENTITY, &MATTE)],
            };
            center_of(&device, &queue, &frame(Sun::UNLIT, vec![batch]))
        };

        let background = center_of(&device, &queue, &frame(Sun::UNLIT, Vec::new()));
        assert_ne!(shot(0.0), background);
        assert_eq!(shot(0.8), background);
    }

    #[test]
    fn a_probe_volume_darkens_sky_light_only_inside_its_box() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sky = Sky {
            zenith: Vec3::splat(0.5),
            horizon: Vec3::splat(0.5),
            ground: Vec3::splat(0.5),
        };
        let shot = |center: Vec3| {
            let volume = ProbeVolume::from_data(probes::ProbeData {
                dimensions: UVec3::splat(3),
                origin: center - Vec3::splat(0.5),
                spacing: 0.5,
                visibility: vec![[128, 128, 128, 26]; 27],
                bounce: vec![[0; 4]; 27],
            });
            let frame = Frame {
                sky: Some(sky),
                probes: Some(volume),
                ..frame(DARK_SUN, vec![near_surface(MATTE)])
            };
            center_of(&device, &queue, &frame)[0]
        };

        let blocked = shot(Vec3::Z * 0.5);
        let outside = shot(Vec3::X * 10.0);
        assert!(
            u16::from(outside) > u16::from(blocked) * 2,
            "outside {outside}, blocked {blocked}"
        );
    }

    #[test]
    fn point_and_spot_lights_reach_only_within_their_range_and_cone() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let shot = |light: LightUniform| {
            let mut frame = frame(DARK_SUN, vec![near_surface(MATTE)]);
            frame.effects.lights = vec![light];
            center_of(&device, &queue, &frame)[0]
        };
        let at = |z: f32| Transform::at(Vec3::Z * z);
        let point = |range| crate::PointLight {
            color: Vec3::ONE,
            intensity: 1.0,
            range,
        };

        let near = shot(LightUniform::point(&at(1.5), &point(10.0)));
        let farther = shot(LightUniform::point(&at(2.5), &point(10.0)));
        let out_of_range = shot(LightUniform::point(&at(1.5), &point(0.9)));
        assert!(near > farther + 20 && farther > 20, "{near} {farther}");
        assert_eq!(out_of_range, 0);

        let spot = crate::SpotLight {
            color: Vec3::ONE,
            intensity: 1.0,
            ..crate::SpotLight::default()
        };
        let toward = shot(LightUniform::spot(&at(1.5), &spot));
        let away = Transform {
            rotation: Quat::from_rotation_x(std::f32::consts::PI),
            ..at(1.5)
        };
        assert!(toward > 100, "{toward}");
        assert_eq!(shot(LightUniform::spot(&away, &spot)), 0);
    }

    #[test]
    fn a_mirror_inside_a_reflection_probe_shows_its_surroundings_the_right_way_up() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        // Tilted so that the view, along -Z, reflects toward +X and a little up.
        let normal = Vec3::new(1.0, 0.3, 1.0).normalize();
        let mirror = quad(
            Vec3::Z,
            [1.0; 3],
            Mat4::from_rotation_translation(
                Quat::from_rotation_arc(Vec3::Z, normal),
                Vec3::Z * 0.5,
            ),
            Material {
                metallic: 1.0,
                roughness: 0.0,
                ..MATTE
            },
        );
        let panel = |y: f32, emissive: Vec3| {
            let facing_minus_x = Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2);
            let transform = Mat4::from_translation(Vec3::new(3.0, y, 0.0)) * facing_minus_x;
            quad(Vec3::Z, [1.0; 3], transform, Material { emissive, ..MATTE })
        };
        let surroundings = vec![
            mirror.clone(),
            panel(1.0, Vec3::new(2.0, 0.0, 0.0)),
            panel(-1.0, Vec3::new(0.0, 0.0, 2.0)),
        ];
        let shot = |probe: bool| {
            let mut frame = frame(DARK_SUN, vec![mirror.clone()]);
            frame.effects.reflection = probe.then(|| Reflection {
                id: crate::reflection::ReflectionId {
                    entity: 1,
                    revision: 0,
                    center: Vec3::Z,
                    size: Vec3::splat(10.0),
                },
                faces: Some(Box::new(std::array::from_fn(|_| surroundings.clone()))),
            });
            center_of(&device, &queue, &frame)
        };

        let without = shot(false);
        let with = shot(true);
        assert!(with[0] > without[0] + 60, "{with:?} {without:?}");
        assert!(with[0] > with[2] * 2, "{with:?}");
    }

    #[test]
    fn blocked_sky_light_returns_tinted_by_what_blocks_it() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let shot = |bounce: [u8; 4]| {
            let volume = ProbeVolume::from_data(probes::ProbeData {
                dimensions: UVec3::splat(3),
                origin: Vec3::Z * 0.5 - Vec3::splat(0.5),
                spacing: 0.5,
                visibility: vec![[128, 128, 128, 26]; 27],
                bounce: vec![bounce; 27],
            });
            let frame = Frame {
                sky: Some(Sky::default()),
                probes: Some(volume),
                ..frame(DARK_SUN, vec![near_surface(MATTE)])
            };
            center_of(&device, &queue, &frame)
        };

        let without = shot([0; 4]);
        let red = shot([230, 40, 40, 255]);
        assert!(red[0] > without[0] + 20, "{red:?} {without:?}");
        assert!(red[0] > red[1] * 2, "{red:?}");
    }

    #[test]
    fn dropped_meshes_release_their_gpu_copy() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut scene = SceneRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let drawn = frame(
            Sun::UNLIT,
            vec![Batch {
                palette: None,
                mesh: Mesh::triangle(),
                material: Material::default(),
                instances: vec![Instance::new(Mat4::IDENTITY, &Material::default())],
            }],
        );

        scene.draw(&device, &queue, &view, (SIZE, SIZE), &drawn);
        scene.draw(&device, &queue, &view, (SIZE, SIZE), &drawn);
        assert_eq!(scene.meshes.len(), 1);

        drop(drawn);
        scene.draw(
            &device,
            &queue,
            &view,
            (SIZE, SIZE),
            &frame(Sun::UNLIT, Vec::new()),
        );
        assert!(scene.meshes.is_empty());
    }

    #[test]
    fn output_matches_between_srgb_and_unorm_targets() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let scene = placeholder_scene();

        let encoded = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &scene);
        let native = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb, &scene);
        assert!(center_pixel(&encoded)[1] > 64, "the triangle was not drawn");

        let stride = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        for y in 0..SIZE as usize {
            for x in 0..SIZE as usize {
                let at = y * stride + x * 4;
                for channel in 0..3 {
                    assert!(
                        encoded[at + channel].abs_diff(native[at + channel]) <= 1,
                        "pixel ({x}, {y}) channel {channel}: {} vs {}",
                        encoded[at + channel],
                        native[at + channel],
                    );
                }
            }
        }
    }

    fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
        let at = (y * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT + x * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    }

    fn with_quality(mut frame: Frame, quality: Quality) -> Frame {
        frame.effects.quality = quality;
        frame
    }

    fn rendered(device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> Vec<u8> {
        render_to_pixels(device, queue, wgpu::TextureFormat::Rgba8Unorm, frame)
    }

    #[test]
    fn soft_shadows_blur_more_the_farther_the_caster_is_from_the_ground() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let quality = Quality {
            shadow_cascades: 1,
            shadow_map_size: 2048,
            soft_shadows: true,
            ..Quality::default()
        };
        // Counts pixels across the middle row that are neither fully lit nor fully shadowed.
        let penumbra = |gap: f32| {
            let mut world = hecs::World::new();
            world.spawn((
                crate::Camera::default(),
                crate::Transform::looking_at(Vec3::Y * 3.0, Vec3::ZERO, Vec3::Z),
            ));
            world.spawn((Sun {
                direction: Vec3::NEG_Y,
                ambient: Vec3::ZERO,
                ..Sun::default()
            },));
            world.spawn((crate::Transform::default(), Mesh::plane(40.0)));
            world.spawn((
                crate::Transform::at(Vec3::new(2.0, gap + 2.0, 0.0)),
                Mesh::cube(4.0),
            ));
            let frame = crate::game::extract_frame(&world, SIZE, SIZE, 1.0, &quality);
            let pixels = rendered(&device, &queue, &frame);
            let row: Vec<u16> = (0..SIZE)
                .map(|x| u16::from(pixel(&pixels, x, SIZE / 2)[0]))
                .collect();
            let lit = *row.iter().max().unwrap();
            let partial = row
                .iter()
                .filter(|&&v| v > lit / 5 && v < lit * 4 / 5)
                .count();
            (partial, lit)
        };

        let (near, lit) = penumbra(0.5);
        let (far, _) = penumbra(30.0);
        assert!(lit > 100, "{lit}");
        assert!(far > near + 1, "far {far}, near {near}");
    }

    #[test]
    fn a_lower_internal_resolution_keeps_the_image_in_place() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let small = quad(
            Vec3::Z,
            [0.6; 3],
            Mat4::from_scale(Vec3::splat(0.5)),
            Material::default(),
        );
        let scene = frame(Sun::UNLIT, vec![small]);
        let half = Quality {
            render_scale: 0.5,
            ..Quality::default()
        };
        let full = rendered(&device, &queue, &scene);
        let scaled = rendered(&device, &queue, &with_quality(scene, half));
        for (x, y) in [(32, 32), (20, 20), (4, 4), (60, 32)] {
            let difference = pixel(&full, x, y)[0].abs_diff(pixel(&scaled, x, y)[0]);
            assert!(difference < 8, "({x}, {y}) differs by {difference}");
        }
    }

    #[test]
    fn bloom_spreads_only_light_brighter_than_white() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let spot = |brightness| {
            frame(
                Sun::UNLIT,
                vec![quad(
                    Vec3::Z,
                    [brightness; 3],
                    Mat4::from_scale(Vec3::splat(0.15)),
                    Material::default(),
                )],
            )
        };
        let glow = Quality {
            bloom: 0.2,
            ..Quality::default()
        };
        let beside = |frame| pixel(&rendered(&device, &queue, &frame), 32, 44)[0];
        let background = beside(spot(40.0));
        assert!(beside(with_quality(spot(40.0), glow)) > background + 5);
        assert_eq!(beside(with_quality(spot(0.9), glow)), background);
    }

    /// `frame` seen through a camera at the origin looking along -Z.
    fn through_lens(mut frame: Frame) -> Frame {
        let projection = Mat4::perspective_rh(1.0, 1.0, 100.0, 0.1);
        frame.view_projection = projection;
        frame.camera_position = Vec3::ZERO;
        frame.effects.projection = projection;
        frame
    }

    #[test]
    fn antialiasing_softens_a_slanted_edge() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let slanted = |antialias| {
            let turned = Mat4::from_rotation_z(0.4) * Mat4::from_scale(Vec3::splat(0.5));
            let frame = frame(Sun::UNLIT, vec![quad(Vec3::Z, [1.0; 3], turned, MATTE)]);
            let pixels = rendered(
                &device,
                &queue,
                &with_quality(
                    frame,
                    Quality {
                        antialias,
                        ..Quality::default()
                    },
                ),
            );
            (0..SIZE)
                .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                .filter(|&(x, y)| (30..220).contains(&pixel(&pixels, x, y)[0]))
                .count()
        };
        let hard = slanted(false);
        assert!(slanted(true) > hard + 20, "{hard}");
    }

    #[test]
    fn depth_of_field_blurs_only_what_is_out_of_focus() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let side = 16;
        let stripes = (0..side * side)
            .flat_map(|i: u32| {
                if i.is_multiple_of(2) {
                    [255; 4]
                } else {
                    [0, 0, 0, 255]
                }
            })
            .collect();
        let striped = Material {
            base_color_texture: Some(Texture::from_rgba8(side, side, stripes)),
            ..MATTE
        };
        let contrast = |focus_distance| {
            let wall =
                Mat4::from_translation(Vec3::NEG_Z * 2.0) * Mat4::from_scale(Vec3::splat(3.0));
            let mut frame = through_lens(frame(
                Sun::UNLIT,
                vec![quad(Vec3::Z, [1.0; 3], wall, striped.clone())],
            ));
            frame.effects.depth_of_field = Some(DepthOfField {
                focus_distance,
                blur: 0.1,
            });
            let pixels = rendered(&device, &queue, &frame);
            let row: Vec<u8> = (24..40).map(|x| pixel(&pixels, x, 32)[0]).collect();
            row.iter().max().unwrap() - row.iter().min().unwrap()
        };
        let sharp = contrast(2.0);
        let blurred = contrast(20.0);
        assert!(
            sharp > 150 && blurred < sharp / 2,
            "sharp {sharp}, blurred {blurred}"
        );
    }

    #[test]
    fn light_shafts_reach_only_along_clear_paths_to_the_sun() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let beside_sun = |light_shafts, blocked: bool| {
            let batches = if blocked {
                let between = Mat4::from_translation(Vec3::new(0.111, 0.0, -1.0))
                    * Mat4::from_scale(Vec3::splat(0.03));
                vec![quad(Vec3::Z, [0.0; 3], between, MATTE)]
            } else {
                Vec::new()
            };
            let frame = Frame {
                sky: Some(Sky::default()),
                ..through_lens(frame(dim_sun(Vec3::Z), batches))
            };
            let quality = Quality {
                light_shafts,
                ..Quality::default()
            };
            pixel(
                &rendered(&device, &queue, &with_quality(frame, quality)),
                44,
                32,
            )[2]
        };
        let plain = beside_sun(0.0, false);
        let clear = beside_sun(2.0, false);
        assert!(clear > plain + 10, "{plain} {clear}");
        assert!(beside_sun(2.0, true) < clear, "{clear}");
    }

    #[test]
    fn light_shafts_glow_with_the_sun_not_the_sky() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let faint = Sun {
            color: Vec3::splat(0.005),
            ..dim_sun(Vec3::Z)
        };
        let beside_sun = |light_shafts| {
            let frame = Frame {
                sky: Some(Sky::default()),
                ..through_lens(frame(faint, Vec::new()))
            };
            let quality = Quality {
                light_shafts,
                ..Quality::default()
            };
            pixel(
                &rendered(&device, &queue, &with_quality(frame, quality)),
                44,
                32,
            )[2]
        };
        let (plain, shafted) = (beside_sun(0.0), beside_sun(2.0));
        assert!(shafted <= plain + 2, "{plain} {shafted}");
    }

    #[test]
    fn particles_add_light_unless_something_is_in_front() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let sparks = |depth| {
            let mut frame = frame(DARK_SUN, vec![solid_quad([1.0; 3], 0.5)]);
            frame.effects.camera_right = Vec3::X;
            frame.effects.camera_up = Vec3::Y;
            frame.effects.particles = vec![ParticleInstance {
                center: [0.0, 0.0, depth],
                size: 0.5,
                color: [4.0, 4.0, 4.0, 1.0],
            }];
            center_of(&device, &queue, &frame)[0]
        };
        assert!(sparks(0.8) > 100);
        assert_eq!(sparks(0.2), 0);
    }

    #[test]
    fn fog_takes_over_with_distance() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let fogged = |density| {
            let mut frame = frame(Sun::UNLIT, vec![surface(MATTE)]);
            frame.effects.fog = Some(Fog {
                color: Vec3::new(1.0, 0.0, 0.0),
                density,
            });
            center_of(&device, &queue, &frame)
        };
        let clear = fogged(0.0);
        assert_eq!(clear[0], clear[1]);
        let thick = fogged(10.0);
        assert!(thick[0] > 100 && thick[1] < 5, "{thick:?}");
    }

    #[test]
    fn ambient_occlusion_darkens_creases_and_leaves_open_ground() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let wall = quad(
            Vec3::Z,
            [0.5; 3],
            Mat4::from_translation(Vec3::new(-0.5, 0.0, 0.5)) * Mat4::from_scale(Vec3::splat(0.5)),
            Material::default(),
        );
        let ground = quad(
            Vec3::Z,
            [0.5; 3],
            Mat4::from_translation(Vec3::Z * 0.2),
            Material::default(),
        );
        let scene = frame(Sun::UNLIT, vec![wall, ground]);
        let occluded = Quality {
            ambient_occlusion: 1.0,
            ..Quality::default()
        };
        let plain = rendered(&device, &queue, &scene);
        let shaded = rendered(&device, &queue, &with_quality(scene, occluded));
        let crease = (36, 32);
        let open = (60, 60);
        assert!(
            pixel(&shaded, crease.0, crease.1)[0] + 10 < pixel(&plain, crease.0, crease.1)[0],
            "{:?} vs {:?}",
            pixel(&shaded, crease.0, crease.1),
            pixel(&plain, crease.0, crease.1)
        );
        let open_difference =
            pixel(&shaded, open.0, open.1)[0].abs_diff(pixel(&plain, open.0, open.1)[0]);
        assert!(open_difference < 4, "{open_difference}");
    }

    #[test]
    fn ambient_occlusion_leaves_distant_flat_ground_alone_at_any_resolution() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let ground = quad(
            Vec3::Z,
            [0.5; 3],
            Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
                * Mat4::from_scale(Vec3::splat(500.0)),
            Material::default(),
        );
        let projection = Mat4::perspective_rh(1.0, 1.0, 1000.0, 0.1);
        let view = Mat4::look_at_rh(Vec3::Y * 2.0, Vec3::new(0.0, 1.6, -10.0), Vec3::Y);
        let mut scene = frame(Sun::UNLIT, vec![ground]);
        scene.view_projection = projection * view;
        scene.effects.projection = projection;
        // Odd sizes, where coordinates rounded through a fraction of the size land on a neighbor.
        for side in [64.0, 61.0, 55.0, 47.0, 41.0] {
            let render_scale = side / SIZE as f32;
            let quality = |ambient_occlusion| Quality {
                render_scale,
                ambient_occlusion,
                ..Quality::default()
            };
            let plain = rendered(&device, &queue, &with_quality(scene.clone(), quality(0.0)));
            let shaded = rendered(&device, &queue, &with_quality(scene.clone(), quality(1.0)));
            for y in SIZE / 2 + 4..SIZE {
                let darker = pixel(&plain, 32, y)[0].saturating_sub(pixel(&shaded, 32, y)[0]);
                assert!(
                    darker < 4,
                    "row {y} at scale {render_scale} darkens by {darker}"
                );
            }
        }
    }
}
