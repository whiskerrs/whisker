use glam::{Mat4, Vec3, Vec4};

use crate::game::DepthOfField;

use crate::quality::Quality;
use crate::renderer::{DEPTH_FORMAT, HDR_FORMAT};

const BLOOM_LEVELS: u32 = 4;
const OCCLUSION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const SHARPEN_WHEN_UPSCALING: f32 = 0.35;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
    amounts: Vec4,
    output_size: Vec4,
    focus: Vec4,
    sun: Vec4,
    inverse_projection: Mat4,
}

/// What the passes after the scene need to know about the camera.
pub(crate) struct PostView {
    pub(crate) projection: Mat4,
    pub(crate) view_projection: Mat4,
    pub(crate) to_sun: Vec3,
    pub(crate) depth_of_field: Option<DepthOfField>,
}

impl PostView {
    pub(crate) fn reads_depth(&self, quality: &Quality) -> bool {
        quality.ambient_occlusion > 0.0
            || quality.light_shafts > 0.0
            || self.depth_of_field.is_some()
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OcclusionUniform {
    inverse_projection: Mat4,
    projection: Mat4,
}

/// The render targets of one internal resolution.
pub(crate) struct SizedTargets {
    key: (u32, u32, bool),
    size: (u32, u32),
    pub(crate) depth: wgpu::TextureView,
    pub(crate) hdr: wgpu::TextureView,
    bloom_views: Vec<wgpu::TextureView>,
    // The scene, then each bloom level in turn.
    bloom_sources: Vec<wgpu::BindGroup>,
    occlusion: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    post_bindings: wgpu::BindGroup,
}

impl SizedTargets {
    pub(crate) fn matches(&self, size: (u32, u32), quality: &Quality) -> bool {
        self.key == (size.0, size.1, quality.ambient_occlusion > 0.0)
    }
}

/// Everything between the lit scene and the screen: occlusion, bloom, upscaling, and tone mapping.
pub(crate) struct PostPasses {
    post_pipeline: wgpu::RenderPipeline,
    post_layout: wgpu::BindGroupLayout,
    post_uniform: wgpu::Buffer,
    source_layout: wgpu::BindGroupLayout,
    bloom_first: wgpu::RenderPipeline,
    bloom_down: wgpu::RenderPipeline,
    bloom_up: wgpu::RenderPipeline,
    occlusion_pipeline: wgpu::RenderPipeline,
    occlusion_layout: wgpu::BindGroupLayout,
    occlusion_uniform: wgpu::Buffer,
    no_occlusion: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

fn fragment_entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty,
        count: None,
    }
}

const FLOAT_TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: true },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};
const DEPTH_TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Depth,
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};
const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Uniform,
    has_dynamic_offset: false,
    min_binding_size: None,
};

struct FullScreen<'a> {
    label: &'a str,
    layout: &'a wgpu::BindGroupLayout,
    shader: &'a wgpu::ShaderModule,
    entry_point: &'a str,
    constants: &'a [(&'a str, f64)],
    target: wgpu::ColorTargetState,
}

fn full_screen_pipeline(device: &wgpu::Device, pass: FullScreen<'_>) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(pass.label),
        bind_group_layouts: &[pass.layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(pass.label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: pass.shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: pass.shader,
            entry_point: Some(pass.entry_point),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: pass.constants,
                ..Default::default()
            },
            targets: &[Some(pass.target)],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    })
}

fn color_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    })
}

impl PostPasses {
    pub(crate) fn new(
        device: &wgpu::Device,
        no_occlusion: wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Self {
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba post"),
            entries: &[
                fragment_entry(0, FLOAT_TEXTURE),
                fragment_entry(1, filtering),
                fragment_entry(2, FLOAT_TEXTURE),
                fragment_entry(3, FLOAT_TEXTURE),
                fragment_entry(4, UNIFORM),
                fragment_entry(5, DEPTH_TEXTURE),
            ],
        });
        let source_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba bloom"),
            entries: &[
                fragment_entry(0, FLOAT_TEXTURE),
                fragment_entry(1, filtering),
            ],
        });
        let occlusion_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kiba occlusion"),
            entries: &[fragment_entry(0, DEPTH_TEXTURE), fragment_entry(1, UNIFORM)],
        });
        // Encoding in the shader gives the same output on backends without an sRGB surface format.
        let constants = [("ENCODE_SRGB", f64::from(u8::from(!format.is_srgb())))];
        let post_shader = device.create_shader_module(wgpu::include_wgsl!("post.wgsl"));
        let bloom_shader = device.create_shader_module(wgpu::include_wgsl!("bloom.wgsl"));
        let occlusion_shader = device.create_shader_module(wgpu::include_wgsl!("ao.wgsl"));
        let bloom_pipeline = |label, entry_point, blend| {
            full_screen_pipeline(
                device,
                FullScreen {
                    label,
                    layout: &source_layout,
                    shader: &bloom_shader,
                    entry_point,
                    constants: &[],
                    target: wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    },
                },
            )
        };
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let bloom_first = bloom_pipeline("kiba bloom first", "fs_first", None);
        let bloom_down = bloom_pipeline("kiba bloom down", "fs_down", None);
        let bloom_up = bloom_pipeline(
            "kiba bloom up",
            "fs_up",
            Some(wgpu::BlendState {
                color: additive,
                alpha: additive,
            }),
        );
        let uniform = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            post_pipeline: full_screen_pipeline(
                device,
                FullScreen {
                    label: "kiba post",
                    layout: &post_layout,
                    shader: &post_shader,
                    entry_point: "fs_main",
                    constants: &constants,
                    target: format.into(),
                },
            ),
            occlusion_pipeline: full_screen_pipeline(
                device,
                FullScreen {
                    label: "kiba occlusion",
                    layout: &occlusion_layout,
                    shader: &occlusion_shader,
                    entry_point: "fs_main",
                    constants: &[],
                    target: OCCLUSION_FORMAT.into(),
                },
            ),
            post_layout,
            post_uniform: uniform("kiba post", size_of::<PostUniform>()),
            source_layout,
            bloom_first,
            bloom_down,
            bloom_up,
            occlusion_layout,
            occlusion_uniform: uniform("kiba occlusion", size_of::<OcclusionUniform>()),
            no_occlusion,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("kiba post"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    fn source_bindings(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba bloom"),
            layout: &self.source_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    pub(crate) fn targets(
        &self,
        device: &wgpu::Device,
        size: (u32, u32),
        quality: &Quality,
    ) -> SizedTargets {
        let image = |label, size: (u32, u32), mips, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: mips,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let drawn_and_read =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let depth = image("kiba depth", size, 1, DEPTH_FORMAT, drawn_and_read)
            .create_view(&Default::default());
        let hdr =
            image("kiba hdr", size, 1, HDR_FORMAT, drawn_and_read).create_view(&Default::default());

        let half = ((size.0 / 2).max(1), (size.1 / 2).max(1));
        let levels = BLOOM_LEVELS.min(u32::BITS - half.0.max(half.1).leading_zeros());
        let bloom = image("kiba bloom", half, levels, HDR_FORMAT, drawn_and_read);
        let bloom_views: Vec<_> = (0..levels)
            .map(|level| {
                bloom.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let bloom_sources = std::iter::once(&hdr)
            .chain(&bloom_views)
            .map(|view| self.source_bindings(device, view))
            .collect();

        let occlusion = (quality.ambient_occlusion > 0.0).then(|| {
            let view = image("kiba occlusion", half, 1, OCCLUSION_FORMAT, drawn_and_read)
                .create_view(&Default::default());
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("kiba occlusion"),
                layout: &self.occlusion_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.occlusion_uniform.as_entire_binding(),
                    },
                ],
            });
            (view, bindings)
        });
        let post_bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kiba post"),
            layout: &self.post_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&bloom_views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        occlusion
                            .as_ref()
                            .map_or(&self.no_occlusion, |(view, _)| view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.post_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&depth),
                },
            ],
        });
        SizedTargets {
            key: (size.0, size.1, occlusion.is_some()),
            size,
            depth,
            hdr,
            bloom_views,
            bloom_sources,
            occlusion,
            post_bindings,
        }
    }

    /// Runs every pass after the scene, ending in `output`.
    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        targets: &SizedTargets,
        (output, output_size): (&wgpu::TextureView, (u32, u32)),
        quality: &Quality,
        view: &PostView,
    ) {
        let projection = view.projection;
        if let Some((view, bindings)) = &targets.occlusion {
            queue.write_buffer(
                &self.occlusion_uniform,
                0,
                bytemuck::bytes_of(&OcclusionUniform {
                    inverse_projection: projection.inverse(),
                    projection,
                }),
            );
            let mut pass = color_pass(
                encoder,
                "kiba occlusion",
                view,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            pass.set_pipeline(&self.occlusion_pipeline);
            pass.set_bind_group(0, bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        if quality.bloom > 0.0 {
            self.encode_bloom(encoder, targets);
        }
        let sharpen = if quality.render_scale < 1.0 {
            SHARPEN_WHEN_UPSCALING
        } else {
            0.0
        };
        let height = targets.size.1 as f32;
        let focus = view.depth_of_field.map_or(Vec4::ZERO, |focus| {
            Vec4::new(focus.focus_distance, focus.blur * height, 0.0, 0.0)
        });
        // A direction projects with no translation, so the sun lands where the sky shows it.
        let sun = view.view_projection * view.to_sun.extend(0.0);
        let sun = if quality.light_shafts > 0.0 && sun.w > 0.0 {
            let ndc = sun.truncate().truncate() / sun.w;
            Vec4::new(
                ndc.x * 0.5 + 0.5,
                0.5 - ndc.y * 0.5,
                quality.light_shafts,
                0.0,
            )
        } else {
            Vec4::ZERO
        };
        queue.write_buffer(
            &self.post_uniform,
            0,
            bytemuck::bytes_of(&PostUniform {
                amounts: Vec4::new(
                    quality.bloom,
                    quality.ambient_occlusion,
                    sharpen,
                    f32::from(u8::from(quality.antialias)),
                ),
                output_size: Vec4::new(output_size.0 as f32, output_size.1 as f32, 0.0, 0.0),
                focus,
                sun,
                inverse_projection: projection.inverse(),
            }),
        );
        let mut pass = color_pass(
            encoder,
            "kiba post",
            output,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        pass.set_pipeline(&self.post_pipeline);
        pass.set_bind_group(0, &targets.post_bindings, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_bloom(&self, encoder: &mut wgpu::CommandEncoder, targets: &SizedTargets) {
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        for (level, view) in targets.bloom_views.iter().enumerate() {
            let mut pass = color_pass(encoder, "kiba bloom down", view, clear);
            pass.set_pipeline(if level == 0 {
                &self.bloom_first
            } else {
                &self.bloom_down
            });
            pass.set_bind_group(0, &targets.bloom_sources[level], &[]);
            pass.draw(0..3, 0..1);
        }
        for level in (0..targets.bloom_views.len() - 1).rev() {
            let mut pass = color_pass(
                encoder,
                "kiba bloom up",
                &targets.bloom_views[level],
                wgpu::LoadOp::Load,
            );
            pass.set_pipeline(&self.bloom_up);
            pass.set_bind_group(0, &targets.bloom_sources[level + 2], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
