//! The forward pass: its uniforms, bind group layouts and pipelines.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use glam::Mat4;
use whisker_wgpu::{Gpu, wgpu};

use super::resources::{GpuTexture, half, upload_cube, upload_texture};
use super::shadow::{MAX_SHADOW_LAYERS, SHADOW_FORMAT};
use super::{DEPTH_FORMAT, MAX_DIRECTIONAL, MAX_POINT};
use crate::core::{AlphaMode, Material, TextureTransform, linear_rgba, srgb_rgba};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct FrameUniform {
    pub(super) view_proj: [[f32; 4]; 4],
    pub(super) inverse_view_proj: [[f32; 4]; 4],
    pub(super) camera_position: [f32; 4],
    pub(super) camera_forward: [f32; 4],
    pub(super) ambient: [f32; 4],
    pub(super) sky: [f32; 4],
    pub(super) ground: [f32; 4],
    pub(super) counts: [u32; 4],
    pub(super) shadow: [f32; 4],
    pub(super) tone: [f32; 4],
    pub(super) fog_color: [f32; 4],
    pub(super) fog: [f32; 4],
    pub(super) environment: [f32; 4],
    pub(super) irradiance: [[f32; 4]; 9],
    pub(super) directional: [DirectionalUniform; MAX_DIRECTIONAL],
    pub(super) point: [PointUniform; MAX_POINT],
    pub(super) shadow_matrices: [[[f32; 4]; 4]; MAX_SHADOW_LAYERS],
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct DirectionalUniform {
    pub(super) direction: [f32; 4],
    pub(super) color: [f32; 4],
    pub(super) shadow: [f32; 4],
    pub(super) splits: [f32; 4],
    pub(super) texel: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct PointUniform {
    pub(super) position: [f32; 4],
    pub(super) color: [f32; 4],
    pub(super) shadow: [f32; 4],
    pub(super) direction: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct DrawUniform {
    pub(super) model: [[f32; 4]; 4],
    pub(super) normal: [[f32; 4]; 4],
    pub(super) base_color: [f32; 4],
    pub(super) emissive: [f32; 4],
    pub(super) params: [f32; 4],
    pub(super) uv_rows: [[f32; 4]; 2],
    pub(super) alpha: [f32; 4],
}

/// Device-wide objects shared by every scene surface.
pub(super) struct Shared {
    pub(super) frame_layout: wgpu::BindGroupLayout,
    pub(super) draw_layout: wgpu::BindGroupLayout,
    pub(super) texture_layout: wgpu::BindGroupLayout,
    pub(super) pipeline_layout: wgpu::PipelineLayout,
    pub(super) shader: wgpu::ShaderModule,
    pub(super) sampler: wgpu::Sampler,
    pub(super) white: GpuTexture,
    pub(super) flat_normal: GpuTexture,
    pub(super) pipelines: RefCell<HashMap<PipelineKey, wgpu::RenderPipeline>>,
    pub(super) shadow_view_layout: wgpu::BindGroupLayout,
    pub(super) shadow_sampler: wgpu::Sampler,
    pub(super) environment_sampler: wgpu::Sampler,
    /// Bound while a scene has no environment.
    pub(super) no_environment: GpuTexture,
    sky_pipeline_layout: wgpu::PipelineLayout,
    sky_pipelines: RefCell<HashMap<(wgpu::TextureFormat, u32), wgpu::RenderPipeline>>,
    shadow_pipeline_layout: wgpu::PipelineLayout,
    shadow_shader: wgpu::ShaderModule,
    shadow_pipeline: OnceCell<wgpu::RenderPipeline>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PipelineKey {
    pub(super) format: wgpu::TextureFormat,
    pub(super) samples: u32,
    pub(super) blend: bool,
    pub(super) double_sided: bool,
    /// Draws with a mirroring transform, whose triangles wind the other way.
    pub(super) mirrored: bool,
}

/// The texture slots of the shader's material bind group, in binding order.
pub(super) const TEXTURE_SLOTS: usize = 5;

thread_local! {
    pub(super) static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

pub(super) fn shared(gpu: &Gpu) -> Rc<Shared> {
    SHARED.with_borrow_mut(|shared| {
        shared
            .get_or_insert_with(|| Rc::new(Shared::new(gpu)))
            .clone()
    })
}

impl Shared {
    fn new(gpu: &Gpu) -> Shared {
        let device = &gpu.device;
        let uniform = |dynamic: bool, size: usize| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: wgpu::BufferSize::new(size as u64),
            },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene frame"),
            entries: &[
                uniform(false, size_of::<FrameUniform>()),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene draw"),
            entries: &[uniform(true, size_of::<DrawUniform>())],
        });
        let mut texture_entries: Vec<wgpu::BindGroupLayoutEntry> = (0..TEXTURE_SLOTS as u32)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .collect();
        texture_entries.push(wgpu::BindGroupLayoutEntry {
            binding: TEXTURE_SLOTS as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene material"),
            entries: &texture_entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker-scene"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&draw_layout),
                Some(&texture_layout),
            ],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whisker-scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker-scene"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let shadow_view_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("whisker-scene shadow view"),
                entries: &[uniform(true, size_of::<[[f32; 4]; 4]>())],
            });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("whisker-scene shadow"),
                bind_group_layouts: &[
                    Some(&shadow_view_layout),
                    Some(&draw_layout),
                    Some(&texture_layout),
                ],
                immediate_size: 0,
            });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whisker-scene shadow"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shadow.wgsl").into()),
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker-scene shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let environment_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker-scene environment"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let no_environment = upload_cube(gpu, 1, &[vec![[0, 0, 0, half(1.0)]; 6]]);
        let sky_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker-scene sky"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let white = upload_texture(gpu, 1, 1, &[255; 4], false);
        let flat_normal = upload_texture(gpu, 1, 1, &[128, 128, 255, 255], false);
        Shared {
            frame_layout,
            draw_layout,
            texture_layout,
            pipeline_layout,
            shader,
            sampler,
            white,
            flat_normal,
            pipelines: RefCell::new(HashMap::new()),
            shadow_view_layout,
            shadow_sampler,
            environment_sampler,
            no_environment,
            sky_pipeline_layout,
            sky_pipelines: RefCell::new(HashMap::new()),
            shadow_pipeline_layout,
            shadow_shader,
            shadow_pipeline: OnceCell::new(),
        }
    }

    /// Fills the background with the environment, behind everything drawn after it.
    pub(super) fn sky_pipeline(
        &self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> wgpu::RenderPipeline {
        self.sky_pipelines
            .borrow_mut()
            .entry((format, samples))
            .or_insert_with(|| {
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("whisker-scene sky"),
                        layout: Some(&self.sky_pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.shader,
                            entry_point: Some("vs_sky"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &self.shader,
                            entry_point: Some("fs_sky"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: None,
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH_FORMAT,
                            depth_write_enabled: Some(false),
                            depth_compare: Some(wgpu::CompareFunction::Always),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: wgpu::MultisampleState {
                            count: samples,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }

    /// Renders casters' depth from a light, culling no faces so open and inside-out meshes cast too.
    pub(super) fn shadow_pipeline(&self, gpu: &Gpu) -> &wgpu::RenderPipeline {
        self.shadow_pipeline.get_or_init(|| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("whisker-scene shadow"),
                    layout: Some(&self.shadow_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.shadow_shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_layout())],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shadow_shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[],
                    }),
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: SHADOW_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: wgpu::DepthBiasState {
                            constant: 2,
                            slope_scale: 2.0,
                            clamp: 0.0,
                        },
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
        })
    }

    pub(super) fn pipeline(&self, gpu: &Gpu, key: PipelineKey) -> wgpu::RenderPipeline {
        let PipelineKey {
            format,
            samples,
            blend,
            double_sided,
            mirrored,
        } = key;
        self.pipelines
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("whisker-scene"),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.shader,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[Some(vertex_layout())],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &self.shader,
                            entry_point: Some("fs"),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: wgpu::PrimitiveState {
                            cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                            front_face: if mirrored {
                                wgpu::FrontFace::Cw
                            } else {
                                wgpu::FrontFace::Ccw
                            },
                            ..Default::default()
                        },
                        depth_stencil: Some(wgpu::DepthStencilState {
                            format: DEPTH_FORMAT,
                            depth_write_enabled: Some(!blend),
                            depth_compare: Some(wgpu::CompareFunction::Less),
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: wgpu::MultisampleState {
                            count: samples,
                            ..Default::default()
                        },
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }
}

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x4,
        4 => Float32x4,
    ];
    wgpu::VertexBufferLayout {
        array_stride: size_of::<crate::core::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

pub(super) fn draw_buffer(
    gpu: &Gpu,
    shared: &Shared,
    stride: u64,
    capacity: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker-scene draws"),
        size: stride * capacity,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whisker-scene draws"),
        layout: &shared.draw_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(size_of::<DrawUniform>() as u64),
            }),
        }],
    });
    (buffer, bind_group)
}

pub(super) fn draw_uniform(model: Mat4, material: &Material, receive_shadow: bool) -> DrawUniform {
    let normal = model.inverse().transpose();
    let identity = TextureTransform::default().rows();
    let (base_color, emissive, params, uv_rows, alpha) = match material {
        Material::Unlit { color } => (
            linear_rgba(*color),
            [0.0; 4],
            [0.0, 1.0, 1.0, 0.0],
            identity,
            [ALPHA_BLEND, 0.0, 0.0, 0.0],
        ),
        Material::Standard(standard) => {
            let [r, g, b, _] = linear_rgba(standard.emissive);
            let k = standard.emissive_intensity;
            let (mode, cutoff) = match standard.alpha_mode {
                AlphaMode::Opaque => (ALPHA_OPAQUE, 0.0),
                AlphaMode::Mask(cutoff) => (ALPHA_MASK, cutoff),
                AlphaMode::Blend => (ALPHA_BLEND, 0.0),
            };
            (
                linear_rgba(standard.base_color),
                [r * k, g * k, b * k, 0.0],
                [standard.metallic, standard.roughness, 0.0, 0.0],
                standard.texture_transform.rows(),
                [mode, cutoff, 0.0, 0.0],
            )
        }
    };
    let mut alpha = alpha;
    alpha[2] = f32::from(u8::from(receive_shadow));
    DrawUniform {
        model: model.to_cols_array_2d(),
        normal: normal.to_cols_array_2d(),
        base_color,
        emissive,
        params,
        uv_rows,
        alpha,
    }
}

const ALPHA_OPAQUE: f32 = 0.0;
const ALPHA_MASK: f32 = 1.0;
const ALPHA_BLEND: f32 = 2.0;

pub(crate) fn translucent(material: &Material) -> bool {
    match material {
        Material::Unlit { color } => srgb_rgba(*color)[3] < 1.0,
        Material::Standard(standard) => {
            standard.alpha_mode == AlphaMode::Blend
                && (srgb_rgba(standard.base_color)[3] < 1.0
                    || standard
                        .base_color_texture
                        .as_ref()
                        .is_some_and(|texture| texture.data().translucent))
        }
    }
}

pub(super) fn double_sided(material: &Material) -> bool {
    matches!(material, Material::Standard(standard) if standard.double_sided)
}
