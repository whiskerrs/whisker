//! Post-processing: the scene drawn into an intermediate image, then bloom, ambient occlusion,
//! depth of field, tone mapping, color grading and FXAA on the way to the surface.

use std::cell::RefCell;
use std::collections::HashMap;

use glam::Mat4;
use whisker_wgpu::{Gpu, wgpu};

use super::resources::{GpuTexture, deformed_vertices, mesh, texture};
use super::shadow::{MAX_SHADOW_LAYERS, SHADOW_FORMAT};
use super::visibility::Ordered;
use super::{Draw, MSAA_SAMPLES, SceneRenderer};
use crate::core::{PostProcessing, ToneMapping};

const BLOOM_LEVELS: usize = 6;
const LDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
    projection: [[f32; 4]; 4],
    inverse_projection: [[f32; 4]; 4],
    bloom: [f32; 4],
    occlusion: [f32; 4],
    focus: [f32; 4],
    tone: [f32; 4],
    output: [f32; 4],
}

/// Device-wide post-processing objects, created the first time a scene uses an effect.
pub(super) struct PostShared {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    no_depth: wgpu::TextureView,
    pipelines: RefCell<HashMap<(&'static str, wgpu::TextureFormat, bool), wgpu::RenderPipeline>>,
}

impl PostShared {
    pub(super) fn new(gpu: &Gpu) -> PostShared {
        let device = &gpu.device;
        let float = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<PostUniform>() as u64),
                    },
                    count: None,
                },
                float(1),
                float(2),
                float(3),
                float(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker-scene post"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whisker-scene post"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("color.wgsl"), include_str!("post.wgsl")).into(),
            ),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker-scene post"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let no_depth = target(gpu, "whisker-scene no depth", (1, 1), SHADOW_FORMAT);
        PostShared {
            layout,
            pipeline_layout,
            shader,
            sampler,
            no_depth,
            pipelines: RefCell::new(HashMap::new()),
        }
    }

    fn pipeline(
        &self,
        gpu: &Gpu,
        entry: &'static str,
        format: wgpu::TextureFormat,
        additive: bool,
    ) -> wgpu::RenderPipeline {
        self.pipelines
            .borrow_mut()
            .entry((entry, format, additive))
            .or_insert_with(|| {
                let add = wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                };
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.shader,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &self.shader,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: additive.then_some(wgpu::BlendState {
                                    color: add,
                                    alpha: add,
                                }),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    })
            })
            .clone()
    }
}

fn target(
    gpu: &Gpu,
    label: &str,
    (width, height): (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The scene's intermediate format: half floats where they can be rendered to, else sRGB bytes,
/// which keep dark gradients smooth but clip bloom's input at white.
pub(super) fn scene_format(gpu: &Gpu) -> (wgpu::TextureFormat, u32) {
    let features = |format| gpu.adapter.get_texture_format_features(format);
    let hdr = wgpu::TextureFormat::Rgba16Float;
    let hdr_features = features(hdr);
    let format = if hdr_features
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        && hdr_features
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
    {
        hdr
    } else {
        wgpu::TextureFormat::Rgba8UnormSrgb
    };
    let multisample = |format| features(format).flags.sample_count_supported(MSAA_SAMPLES);
    let samples = if multisample(format) && multisample(super::DEPTH_FORMAT) {
        MSAA_SAMPLES
    } else {
        1
    };
    (format, samples)
}

/// Full-size and bloom-size images post-processing renders between, remade on resize.
pub(super) struct PostTargets {
    size: (u32, u32),
    pub(super) format: wgpu::TextureFormat,
    pub(super) samples: u32,
    pub(super) scene: wgpu::TextureView,
    pub(super) depth: wgpu::TextureView,
    occlusion: wgpu::TextureView,
    occlusion_blurred: wgpu::TextureView,
    ldr: wgpu::TextureView,
    bloom: Vec<wgpu::TextureView>,
    uniform: wgpu::Buffer,
}

impl PostTargets {
    pub(super) fn new(gpu: &Gpu, size: (u32, u32)) -> PostTargets {
        let (format, samples) = scene_format(gpu);
        let mut bloom = Vec::new();
        let mut level = ((size.0 / 2).max(1), (size.1 / 2).max(1));
        while bloom.len() < BLOOM_LEVELS && (bloom.is_empty() || level.0.min(level.1) >= 4) {
            bloom.push(target(gpu, "whisker-scene bloom", level, format));
            level = ((level.0 / 2).max(1), (level.1 / 2).max(1));
        }
        PostTargets {
            size,
            format,
            samples,
            scene: target(gpu, "whisker-scene scene", size, format),
            depth: target(gpu, "whisker-scene scene depth", size, SHADOW_FORMAT),
            occlusion: target(gpu, "whisker-scene occlusion", size, LDR_FORMAT),
            occlusion_blurred: target(gpu, "whisker-scene occlusion", size, LDR_FORMAT),
            ldr: target(gpu, "whisker-scene antialiasing", size, LDR_FORMAT),
            bloom,
            uniform: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("whisker-scene post"),
                size: size_of::<PostUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    pub(super) fn size(&self) -> (u32, u32) {
        self.size
    }
}

/// Inputs one post pass binds; unset ones bind placeholders.
#[derive(Default)]
struct Inputs<'a> {
    source: Option<&'a wgpu::TextureView>,
    bloom: Option<&'a wgpu::TextureView>,
    occlusion: Option<&'a wgpu::TextureView>,
    lookup: Option<&'a wgpu::TextureView>,
    depth: Option<&'a wgpu::TextureView>,
}

impl SceneRenderer {
    /// Draws the opaque draws' depth from the camera, for passes that read the scene's depth.
    pub(super) fn encode_depth_prepass(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        draws: &[Draw],
        order: &[Ordered],
        view_proj: Mat4,
    ) {
        let Some(targets) = &self.post else {
            return;
        };
        let stride = self.shadow_view_stride;
        let slot = stride * MAX_SHADOW_LAYERS as u64;
        let columns = view_proj.to_cols_array();
        gpu.queue.write_buffer(
            &self.shadow_view_buffer,
            slot,
            bytemuck::cast_slice(&columns),
        );
        let shared = self.shared.clone();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("whisker-scene depth prepass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(shared.depth_pipeline(gpu));
        pass.set_bind_group(0, &self.shadow_view_bind_group, &[slot as u32]);
        for ordered in order.iter().filter(|ordered| !ordered.blended) {
            let draw = &draws[ordered.index];
            let mesh = mesh(gpu, &draw.geometry);
            if mesh.count == 0 {
                continue;
            }
            let deformed = draw
                .deform
                .as_ref()
                .map(|deform| deformed_vertices(gpu, deform));
            let offset = (ordered.index as u64 * self.draw_stride) as u32;
            pass.set_bind_group(1, &self.draw_bind_group, &[offset]);
            pass.set_bind_group(
                2,
                self.materials.bind_group(gpu, &shared, &draw.material),
                &[],
            );
            match &deformed {
                Some(buffer) => pass.set_vertex_buffer(0, buffer.slice(..)),
                None => pass.set_vertex_buffer(0, mesh.vertices.slice(..)),
            }
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
    }

    pub(super) fn encode_post(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        settings: &PostProcessing,
        projection: Mat4,
        (tone_mapping, exposure): (ToneMapping, f32),
    ) {
        let shared = self.shared.clone();
        let post_shared = shared.post(gpu);
        let Some(targets) = &self.post else {
            return;
        };
        let lookup = settings
            .color_grading
            .as_ref()
            .map(|lut| texture(gpu, lut, false));
        let bloom = settings.bloom.unwrap_or_default();
        let occlusion = settings.ambient_occlusion.unwrap_or_default();
        let focus = settings.depth_of_field.unwrap_or_default();
        let encode_output = !self.format.is_srgb();
        let uniform = PostUniform {
            projection: projection.to_cols_array_2d(),
            inverse_projection: projection.inverse().to_cols_array_2d(),
            bloom: [
                bloom.threshold,
                bloom.threshold * 0.5,
                bloom.intensity,
                0.5 + bloom.radius.clamp(0.0, 1.0) * 1.5,
            ],
            occlusion: [occlusion.radius.max(1e-3), occlusion.intensity, 0.0, 0.0],
            focus: [
                focus.focus_distance,
                focus.focus_range,
                focus.max_blur * 0.5,
                f32::from(u8::from(settings.depth_of_field.is_some())),
            ],
            tone: [
                exposure,
                tone_curve(tone_mapping),
                settings
                    .color_grading
                    .as_ref()
                    .map_or(0.0, |lut| lut.height() as f32),
                f32::from(u8::from(settings.bloom.is_some())),
            ],
            output: [
                f32::from(u8::from(settings.fxaa || encode_output)),
                f32::from(u8::from(encode_output)),
                f32::from(u8::from(settings.ambient_occlusion.is_some())),
                0.0,
            ],
        };
        gpu.queue
            .write_buffer(&targets.uniform, 0, bytemuck::bytes_of(&uniform));
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    entry: &'static str,
                    view: &wgpu::TextureView,
                    format: wgpu::TextureFormat,
                    additive: bool,
                    inputs: Inputs<'_>| {
            let white = &shared.white.view;
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(entry),
                layout: &post_shared.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: targets.uniform.as_entire_binding(),
                    },
                    view_entry(1, inputs.source.unwrap_or(white)),
                    view_entry(2, inputs.bloom.unwrap_or(white)),
                    view_entry(3, inputs.occlusion.unwrap_or(white)),
                    view_entry(4, inputs.lookup.unwrap_or(white)),
                    view_entry(5, inputs.depth.unwrap_or(&post_shared.no_depth)),
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&post_shared.sampler),
                    },
                ],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(entry),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if additive {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&post_shared.pipeline(gpu, entry, format, additive));
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        };
        let reads_depth = settings.ambient_occlusion.is_some() || settings.depth_of_field.is_some();
        let depth = reads_depth.then_some(&targets.depth);
        if settings.ambient_occlusion.is_some() {
            pass(
                encoder,
                "fs_occlusion",
                &targets.occlusion,
                LDR_FORMAT,
                false,
                Inputs {
                    depth,
                    ..Inputs::default()
                },
            );
            pass(
                encoder,
                "fs_blur",
                &targets.occlusion_blurred,
                LDR_FORMAT,
                false,
                Inputs {
                    source: Some(&targets.occlusion),
                    ..Inputs::default()
                },
            );
        }
        if settings.bloom.is_some() {
            let levels = &targets.bloom;
            pass(
                encoder,
                "fs_bloom_prefilter",
                &levels[0],
                targets.format,
                false,
                Inputs {
                    source: Some(&targets.scene),
                    ..Inputs::default()
                },
            );
            for level in 1..levels.len() {
                pass(
                    encoder,
                    "fs_bloom_down",
                    &levels[level],
                    targets.format,
                    false,
                    Inputs {
                        source: Some(&levels[level - 1]),
                        ..Inputs::default()
                    },
                );
            }
            for level in (0..levels.len() - 1).rev() {
                pass(
                    encoder,
                    "fs_bloom_up",
                    &levels[level],
                    targets.format,
                    true,
                    Inputs {
                        source: Some(&levels[level + 1]),
                        ..Inputs::default()
                    },
                );
            }
        }
        let composite_target = if settings.fxaa { &targets.ldr } else { output };
        let composite_format = if settings.fxaa {
            LDR_FORMAT
        } else {
            self.format
        };
        pass(
            encoder,
            "fs_composite",
            composite_target,
            composite_format,
            false,
            Inputs {
                source: Some(&targets.scene),
                bloom: settings.bloom.is_some().then(|| &targets.bloom[0]),
                occlusion: settings
                    .ambient_occlusion
                    .is_some()
                    .then_some(&targets.occlusion_blurred),
                lookup: lookup.map(|lut: &GpuTexture| &lut.view),
                depth,
            },
        );
        if settings.fxaa {
            pass(
                encoder,
                "fs_fxaa",
                output,
                self.format,
                false,
                Inputs {
                    source: Some(&targets.ldr),
                    ..Inputs::default()
                },
            );
        }
    }
}

fn view_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

pub(super) fn tone_curve(tone_mapping: ToneMapping) -> f32 {
    match tone_mapping {
        ToneMapping::None => 0.0,
        ToneMapping::Reinhard => 1.0,
        ToneMapping::Aces => 2.0,
        ToneMapping::AgX => 3.0,
    }
}
