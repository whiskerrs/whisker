use std::collections::HashMap;
use std::sync::{LazyLock, Weak};

use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use crate::mesh::{MeshData, Vertex};
use crate::sky::{Sky, SkyRenderer};
use crate::texture::{TextureData, mip_chain};
use crate::{Material, Mesh, Sun, Texture};

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
}

impl Instance {
    pub(crate) fn new(transform: Mat4, material: &Material) -> Self {
        Self {
            transform: transform.to_cols_array_2d(),
            base_color: material.base_color.to_array(),
            metallic: material.metallic,
            roughness: material.roughness,
        }
    }

    #[cfg(test)]
    pub(crate) fn origin(&self) -> [f32; 3] {
        let [x, y, z, _] = self.transform[3];
        [x, y, z]
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Batch {
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
    pub(crate) shadows: Option<Shadows>,
    pub(crate) batches: Vec<Batch>,
}

pub(crate) const CASCADES: usize = 2;
pub(crate) const SHADOW_MAP_SIZE: u32 = 1024;

#[derive(Clone, Debug)]
pub(crate) struct Shadows {
    pub(crate) view_projections: [Mat4; CASCADES],
    pub(crate) split: f32,
    pub(crate) casters: [Vec<Batch>; CASCADES],
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
        shadows: None,
        batches: vec![Batch {
            mesh: TRIANGLE.clone(),
            material: Material::default(),
            instances: vec![Instance::new(Mat4::IDENTITY, &Material::default())],
        }],
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
    cascades: [Mat4; CASCADES],
    shadow_params: Vec4,
}

impl From<&Frame> for FrameUniform {
    fn from(frame: &Frame) -> Self {
        Self {
            view_projection: frame.view_projection,
            // The spare component keeps the uniform to four vectors.
            camera_position: frame.camera_position.extend(frame.exposure),
            to_sun: (-frame.sun.direction).normalize_or_zero().extend(0.0),
            sun_color: frame.sun.color.extend(0.0),
            ambient: frame.sun.ambient.extend(0.0),
            cascades: frame
                .shadows
                .as_ref()
                .map_or([Mat4::IDENTITY; CASCADES], |s| s.view_projections),
            shadow_params: Vec4::new(
                frame.shadows.as_ref().map_or(0.0, |s| s.split),
                f32::from(u8::from(frame.shadows.is_some())),
                f32::from(u8::from(frame.sky.is_some())),
                0.0,
            ),
        }
    }
}

struct GpuMesh {
    source: Weak<MeshData>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
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
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kiba indices"),
                contents: bytemuck::cast_slice(&data.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: data.indices.len() as u32,
        }
    }
}

struct GpuTexture {
    source: Weak<TextureData>,
    view: wgpu::TextureView,
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

struct SizedTargets {
    size: (u32, u32),
    depth: wgpu::TextureView,
    hdr: wgpu::TextureView,
    post_bindings: wgpu::BindGroup,
}

impl SizedTargets {
    fn new(device: &wgpu::Device, post_layout: &wgpu::BindGroupLayout, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kiba depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let hdr = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("kiba hdr"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let post_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba post"),
            layout: post_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&hdr),
            }],
        });
        Self {
            size,
            depth: texture.create_view(&Default::default()),
            hdr,
            post_bindings,
        }
    }
}

/// Encodes the scene into any render target, on a device owned by the caller.
pub struct SceneRenderer {
    pipeline: wgpu::RenderPipeline,
    post_pipeline: wgpu::RenderPipeline,
    post_layout: wgpu::BindGroupLayout,
    frame_uniform: wgpu::Buffer,
    frame_bindings: wgpu::BindGroup,
    sky: SkyRenderer,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_layers: [wgpu::TextureView; CASCADES],
    shadow_uniforms: [wgpu::Buffer; CASCADES],
    shadow_bindings: [wgpu::BindGroup; CASCADES],
    meshes: HashMap<usize, GpuMesh>,
    material_layout: wgpu::BindGroupLayout,
    material_sampler: wgpu::Sampler,
    default_views: [wgpu::TextureView; 3],
    textures: HashMap<(usize, bool), GpuTexture>,
    materials: HashMap<[usize; 3], GpuMaterial>,
    instances: Option<wgpu::Buffer>,
    targets: Option<SizedTargets>,
}

impl SceneRenderer {
    /// Prepares the pipeline for targets of `format`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // Encoding in the shader gives the same output on backends without an sRGB surface format.
        let encode_srgb = !format.is_srgb();
        let constants = [("ENCODE_SRGB", f64::from(u8::from(encode_srgb)))];
        let instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4,
            5 => Float32x4,
            6 => Float32x4,
            7 => Float32x4,
            8 => Float32x3,
            9 => Float32,
            10 => Float32,
        ];
        let shader = device.create_shader_module(wgpu::include_wgsl!("mesh.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kiba mesh"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32x3,
                            2 => Float32x3,
                            3 => Float32x2,
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attributes,
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: DEPTH_COMPARE,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let shadow_shader = device.create_shader_module(wgpu::include_wgsl!("shadow.wgsl"));
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kiba shadow"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instance_attributes,
                    },
                ],
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
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
        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kiba shadow map"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_SIZE,
                height: SHADOW_MAP_SIZE,
                depth_or_array_layers: CASCADES as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_array = shadow_texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let shadow_layers = [0, 1].map(|layer| {
            shadow_texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kiba shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_uniforms = [0, 1].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kiba shadow light"),
                size: size_of::<Mat4>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let shadow_bindings = [0, 1].map(|cascade: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("kiba shadow"),
                layout: &shadow_pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: shadow_uniforms[cascade].as_entire_binding(),
                }],
            })
        });
        let post_shader = device.create_shader_module(wgpu::include_wgsl!("post.wgsl"));
        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kiba post"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &post_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &post_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kiba frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky = SkyRenderer::new(device);
        let sky_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kiba sky"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let frame_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba frame"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_array),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&sky.radiance_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&sky.irradiance_view()),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&sky_sampler),
                },
            ],
        });
        Self {
            frame_uniform,
            frame_bindings,
            sky,
            shadow_pipeline,
            shadow_layers,
            shadow_uniforms,
            shadow_bindings,
            meshes: HashMap::new(),
            material_layout: pipeline.get_bind_group_layout(1),
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
            ],
            textures: HashMap::new(),
            materials: HashMap::new(),
            instances: None,
            targets: None,
            post_layout: post_pipeline.get_bind_group_layout(0),
            post_pipeline,
            pipeline,
        }
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
        for batch in batches {
            self.meshes
                .entry(batch.mesh.key())
                .or_insert_with(|| GpuMesh::new(device, &batch.mesh));
            self.prepare_material(device, queue, &batch.material);
        }
        for batch in casters.clone() {
            self.meshes
                .entry(batch.mesh.key())
                .or_insert_with(|| GpuMesh::new(device, &batch.mesh));
        }
        if self
            .targets
            .as_ref()
            .is_none_or(|targets| targets.size != size)
        {
            self.targets = Some(SizedTargets::new(device, &self.post_layout, size));
        }
        let targets = self.targets.as_ref().expect("allocated above");

        let instance_data: Vec<Instance> = batches
            .iter()
            .chain(casters)
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
        queue.write_buffer(
            &self.frame_uniform,
            0,
            bytemuck::bytes_of(&FrameUniform::from(frame)),
        );

        self.sky.prepare(device, queue, frame);

        let mut encoder = device.create_command_encoder(&Default::default());
        let mut next_instance: u64 = batches.iter().map(|b| b.instances.len() as u64).sum();
        if let Some(shadows) = &frame.shadows {
            for cascade in 0..CASCADES {
                queue.write_buffer(
                    &self.shadow_uniforms[cascade],
                    0,
                    bytemuck::bytes_of(&shadows.view_projections[cascade]),
                );
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("kiba shadow"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_layers[cascade],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.shadow_pipeline);
                pass.set_bind_group(0, &self.shadow_bindings[cascade], &[]);
                let stride = size_of::<Instance>() as u64;
                let mut first = next_instance;
                for batch in &shadows.casters[cascade] {
                    let count = batch.instances.len() as u64;
                    let mesh = &self.meshes[&batch.mesh.key()];
                    let range = first * stride..(first + count) * stride;
                    first += count;
                    if count == 0 || mesh.index_count == 0 {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_vertex_buffer(1, instances.slice(range));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..count as u32);
                }
                next_instance = first;
            }
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
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.frame_bindings, &[]);
            let mut first = 0;
            for batch in batches {
                let count = batch.instances.len() as u64;
                let mesh = &self.meshes[&batch.mesh.key()];
                let stride = size_of::<Instance>() as u64;
                let range = first * stride..(first + count) * stride;
                first += count;
                // Slicing an empty range panics in wgpu.
                if count == 0 || mesh.index_count == 0 {
                    continue;
                }
                let material = &self.materials[&batch.material.texture_keys()];
                pass.set_bind_group(1, &material.bindings, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, instances.slice(range));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..count as u32);
            }
            if frame.sky.is_some() {
                self.sky.draw(&mut pass);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kiba post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.post_pipeline);
            pass.set_bind_group(0, &targets.post_bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    fn prepare_material(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: &Material,
    ) {
        let key = material.texture_keys();
        if self.materials.contains_key(&key) {
            return;
        }
        // Only base color is sRGB, and an image may fill several slots.
        let slots = [
            (&material.base_color_texture, true),
            (&material.metallic_roughness_texture, false),
            (&material.normal_texture, false),
        ];
        let mut sources = Vec::new();
        for (texture, srgb) in slots {
            if let Some(texture) = texture {
                sources.push(texture.downgrade());
                self.textures
                    .entry((texture.key(), srgb))
                    .or_insert_with(|| GpuTexture::new(device, queue, texture, srgb));
            }
        }
        let views = [0, 1, 2].map(|slot| {
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

    /// Draws at the given physical size, reconfiguring the surface when it changed.
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
            shadows: None,
            batches,
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
        base_color_texture: None,
        metallic_roughness_texture: None,
        normal_texture: None,
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
            let frame = crate::game::extract_frame(&world, SIZE, SIZE, 1.0);
            center_of(&device, &queue, &frame)[0]
        };

        let lit = ground_brightness(false);
        let shadowed = ground_brightness(true);
        assert!(lit > shadowed + 32, "lit {lit}, shadowed {shadowed}");
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

        let matte = shot(vec![surface(MATTE)], Some(red));
        assert!(matte[0] > 100 && matte[1] == 0, "{matte:?}");
        let mirror = Material {
            metallic: 1.0,
            roughness: 0.1,
            ..MATTE
        };
        let reflecting = shot(vec![surface(mirror)], Some(red));
        assert!(reflecting[0] > 100 && reflecting[1] == 0, "{reflecting:?}");

        assert_eq!(shot(vec![surface(MATTE)], None)[0], 0);
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
            let mut batch = surface(mirror.clone());
            batch.instances = vec![Instance::new(Mat4::from_rotation_x(angle), &mirror)];
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
}
