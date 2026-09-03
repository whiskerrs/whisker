use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glyphon::{Cache, Resolution, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport};
use wgpu::util::DeviceExt;
use wgpu::{
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, CommandEncoderDescriptor,
    CompositeAlphaMode, Device, DeviceDescriptor, FragmentState, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PipelineCompilationOptions, PresentMode, PrimitiveState,
    Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp,
    Surface, SurfaceConfiguration, SurfaceError, SurfaceTarget, TextureFormat, TextureUsages,
    TextureViewDescriptor, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};
use whisker_protocol::{
    GradientStop, LayoutRect, NodeId, PaintBox, PaintImage, ResourceId, Transform,
};

use crate::paint::box_paint::{
    BoxPrimitive, BoxPrimitiveKind, background_gradient_primitive, box_shadow_primitive, lower_box,
    resolve_box_geometry, solid_rect_primitive,
};
use crate::paint::color::{gpu_color, text_color};
use crate::scene::{LogicalClip, PaintCommand, ShapeClipStack};
use crate::text::NativeTextHost;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BackdropUniform {
    direction: [f32; 2],
    radius: f32,
    opacity: f32,
}

struct BackdropGpuPipeline {
    pipeline: RenderPipeline,
    composite_pipeline: RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl BackdropGpuPipeline {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop backdrop layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker Desktop backdrop shader"),
            source: ShaderSource::Wgsl(BACKDROP_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker Desktop backdrop pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let create_pipeline = |label, blend| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    targets: &[Some(ColorTargetState {
                        format,
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState::default(),
                depth_stencil: None,
                multisample: MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipeline = create_pipeline("whisker Desktop backdrop pipeline", None);
        let composite_pipeline = create_pipeline(
            "whisker Desktop opacity group compositor",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop backdrop sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            composite_pipeline,
            layout,
            sampler,
        }
    }

    fn bind_group(
        &self,
        device: &Device,
        source: &wgpu::TextureView,
        uniform: BackdropUniform,
    ) -> (Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop backdrop uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop backdrop bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        (buffer, bind_group)
    }
}

#[derive(Debug)]
pub(crate) struct GpuError(String);

impl fmt::Display for GpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for GpuError {}

mod renderer;

mod geometry;
mod shaders;

use geometry::*;
pub(crate) use geometry::{GpuRenderer, background_gradient_draw, background_resource_draw};
#[cfg(all(test, feature = "host-conformance"))]
pub(crate) use geometry::{
    render_box_primitives_offscreen, render_clipped_box_primitives_offscreen,
    render_clipped_box_primitives_with_backdrops_offscreen,
};
use shaders::{BACKDROP_SHADER, BOX_SHADER};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BoxVertex {
    position: [f32; 2],
    color: [f32; 4],
    outer_rect: [f32; 4],
    outer_radii_x: [f32; 4],
    outer_radii_y: [f32; 4],
    inner_rect: [f32; 4],
    inner_radii_x: [f32; 4],
    inner_radii_y: [f32; 4],
    border_widths: [f32; 4],
    mode: f32,
    border_colors: [[f32; 4]; 4],
    border_styles: [f32; 4],
    transformed_position: [f32; 4],
}

impl BoxVertex {
    const ATTRIBUTES: [VertexAttribute; 16] = [
        VertexAttribute {
            format: VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 6]>() as u64,
            shader_location: 2,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 10]>() as u64,
            shader_location: 3,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 14]>() as u64,
            shader_location: 4,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 18]>() as u64,
            shader_location: 5,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 22]>() as u64,
            shader_location: 6,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 26]>() as u64,
            shader_location: 7,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 30]>() as u64,
            shader_location: 8,
        },
        VertexAttribute {
            format: VertexFormat::Float32,
            offset: std::mem::size_of::<[f32; 34]>() as u64,
            shader_location: 9,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 35]>() as u64,
            shader_location: 10,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 39]>() as u64,
            shader_location: 11,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 43]>() as u64,
            shader_location: 12,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 47]>() as u64,
            shader_location: 13,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 51]>() as u64,
            shader_location: 14,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 55]>() as u64,
            shader_location: 15,
        },
    ];

    fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ViewportUniform {
    logical_size: [f32; 2],
    physical_size: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipRecordGpu {
    rect: [f32; 4],
    radii_x: [f32; 4],
    radii_y: [f32; 4],
    inverse_transform: [f32; 16],
    axes: [u32; 4],
    path: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipSegmentGpu {
    from_to: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeClipSpanGpu {
    offset: u32,
    count: u32,
    padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LinearGradientDrawGpu {
    start_end: [f32; 4],
    tile_rect: [f32; 4],
    tile_stride: [f32; 4],
    tile_domain: [f32; 4],
    stop_offset: u32,
    stop_count: u32,
    kind: u32,
    geometry_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LinearGradientStopGpu {
    position: f32,
    padding: [f32; 3],
    color: [f32; 4],
}

#[derive(Clone)]
pub(crate) struct LinearGradientDraw {
    start_end: [f32; 4],
    tile_rect: [f32; 4],
    tile_stride: [f32; 4],
    tile_domain: [f32; 4],
    stops: Vec<LinearGradientStopGpu>,
    kind: u32,
    geometry_flags: u32,
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) type ClippedBoxPrimitive = (
    BoxPrimitive,
    LogicalClip,
    Transform,
    ShapeClipStack,
    Option<LinearGradientDraw>,
    Option<ResourceId>,
    bool,
    Vec<(NodeId, f32)>,
);

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) type BackdropCheckpoint = (LayoutRect, f32, LogicalClip);

struct BoxGpuPipeline {
    pipeline: RenderPipeline,
    viewport_buffer: Buffer,
    viewport_bind_group: wgpu::BindGroup,
    shape_clip_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    fallback_image: GpuImageResource,
}

struct GpuImageResource {
    linear_bind_group: wgpu::BindGroup,
    nearest_bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
    intrinsic_size: [f32; 2],
}

#[derive(Clone, Debug)]
pub(crate) struct RasterResource {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

impl RasterResource {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, GpuError> {
        let expected = width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            .map(|count| count as usize)
            .ok_or_else(|| GpuError("Desktop raster dimensions overflow".into()))?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err(GpuError(format!(
                "Desktop raster has {} bytes, expected {expected} for {width}x{height} RGBA8",
                pixels.len()
            )));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

impl BoxGpuPipeline {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let viewport_uniform = ViewportUniform {
            logical_size: [1.0, 1.0],
            physical_size: [1.0, 1.0],
        };
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop logical viewport"),
            contents: bytemuck::bytes_of(&viewport_uniform),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop viewport layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop viewport"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let shape_clip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop shape clip layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker Desktop background image layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let fallback_image = Self::create_image_resource(
            device,
            queue,
            &image_layout,
            "whisker Desktop fallback background image",
            &RasterResource {
                width: 1,
                height: 1,
                pixels: vec![0; 4],
            },
        );
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker Desktop box shader"),
            source: ShaderSource::Wgsl(BOX_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker Desktop box pipeline layout"),
            bind_group_layouts: &[&viewport_layout, &shape_clip_layout, &image_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("whisker Desktop box pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[BoxVertex::layout()],
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        Self {
            pipeline,
            viewport_buffer,
            viewport_bind_group,
            shape_clip_layout,
            image_layout,
            fallback_image,
        }
    }

    fn create_image_resource(
        device: &Device,
        queue: &Queue,
        layout: &wgpu::BindGroupLayout,
        label: &str,
        raster: &RasterResource,
    ) -> GpuImageResource {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: raster.width,
                height: raster.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &raster.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(raster.width * 4),
                rows_per_image: Some(raster.height),
            },
            wgpu::Extent3d {
                width: raster.width,
                height: raster.height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop background image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let linear_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("whisker Desktop pixelated image sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..wgpu::SamplerDescriptor::default()
        });
        let nearest_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop pixelated image bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&nearest_sampler),
                },
            ],
        });
        GpuImageResource {
            linear_bind_group,
            nearest_bind_group,
            _texture: texture,
            intrinsic_size: [raster.width as f32, raster.height as f32],
        }
    }

    fn upload_image(
        &self,
        device: &Device,
        queue: &Queue,
        raster: &RasterResource,
    ) -> GpuImageResource {
        Self::create_image_resource(
            device,
            queue,
            &self.image_layout,
            "whisker Desktop raster background image",
            raster,
        )
    }

    fn update_viewport(&self, queue: &Queue, logical_size: [f32; 2], physical_size: [f32; 2]) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                logical_size,
                physical_size,
            }),
        );
    }

    fn shape_clip_bind_group(&self, device: &Device, draws: &[DrawCommand]) -> wgpu::BindGroup {
        let mut records = Vec::new();
        let mut path_segments = Vec::new();
        let spans = draws
            .iter()
            .map(|draw| {
                let offset = records.len() as u32;
                if let DrawCommand::Quads { shape_clips, .. } = draw {
                    for clip in shape_clips.iter() {
                        let path_offset = path_segments.len() as u32;
                        if let Some(segments) = &clip.path {
                            path_segments.extend(segments.iter().map(|segment| {
                                ShapeClipSegmentGpu {
                                    from_to: [
                                        segment.from[0],
                                        segment.from[1],
                                        segment.to[0],
                                        segment.to[1],
                                    ],
                                }
                            }));
                        }
                        records.push(ShapeClipRecordGpu {
                            rect: [clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height],
                            radii_x: clip.radii.horizontal,
                            radii_y: clip.radii.vertical,
                            inverse_transform: clip.inverse_transform.0,
                            axes: [u32::from(clip.horizontal), u32::from(clip.vertical), 0, 0],
                            path: [
                                path_offset,
                                path_segments.len() as u32 - path_offset,
                                u32::from(clip.path.is_some()),
                                u32::from(clip.fill_rule == whisker_protocol::FillRule::EvenOdd),
                            ],
                        });
                    }
                }
                ShapeClipSpanGpu {
                    offset,
                    count: records.len() as u32 - offset,
                    padding: [0; 2],
                }
            })
            .collect::<Vec<_>>();
        if records.is_empty() {
            records.push(ShapeClipRecordGpu::zeroed());
        }
        if path_segments.is_empty() {
            path_segments.push(ShapeClipSegmentGpu::zeroed());
        }
        let spans = if spans.is_empty() {
            vec![ShapeClipSpanGpu::zeroed()]
        } else {
            spans
        };
        let mut gradient_stops = Vec::new();
        let gradient_draws = draws
            .iter()
            .map(|draw| {
                let Some(gradient) = draw.gradient() else {
                    return LinearGradientDrawGpu::zeroed();
                };
                let stop_offset = gradient_stops.len() as u32;
                gradient_stops.extend_from_slice(&gradient.stops);
                LinearGradientDrawGpu {
                    start_end: gradient.start_end,
                    tile_rect: gradient.tile_rect,
                    tile_stride: gradient.tile_stride,
                    tile_domain: gradient.tile_domain,
                    stop_offset,
                    stop_count: gradient.stops.len() as u32,
                    kind: gradient.kind,
                    geometry_flags: gradient.geometry_flags,
                }
            })
            .collect::<Vec<_>>();
        if gradient_stops.is_empty() {
            gradient_stops.push(LinearGradientStopGpu::zeroed());
        }
        let gradient_draws = if gradient_draws.is_empty() {
            vec![LinearGradientDrawGpu::zeroed()]
        } else {
            gradient_draws
        };
        let record_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip records"),
            contents: bytemuck::cast_slice(&records),
            usage: BufferUsages::STORAGE,
        });
        let span_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip spans"),
            contents: bytemuck::cast_slice(&spans),
            usage: BufferUsages::STORAGE,
        });
        let gradient_draw_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop linear gradient draws"),
            contents: bytemuck::cast_slice(&gradient_draws),
            usage: BufferUsages::STORAGE,
        });
        let gradient_stop_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop linear gradient stops"),
            contents: bytemuck::cast_slice(&gradient_stops),
            usage: BufferUsages::STORAGE,
        });
        let path_segment_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop shape clip path segments"),
            contents: bytemuck::cast_slice(&path_segments),
            usage: BufferUsages::STORAGE,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker Desktop shape clips"),
            layout: &self.shape_clip_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: record_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: span_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: gradient_draw_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: gradient_stop_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: path_segment_buffer.as_entire_binding(),
                },
            ],
        })
    }
}

enum DrawCommand {
    BeginOpacityGroup {
        node: NodeId,
        opacity: f32,
    },
    EndOpacityGroup {
        node: NodeId,
    },
    BackdropBlur {
        rect: LayoutRect,
        radius: f32,
        clip: LogicalClip,
    },
    Quads {
        vertices: Range<u32>,
        clip: LogicalClip,
        shape_clips: ShapeClipStack,
        gradient: Option<LinearGradientDraw>,
        resource: Option<ResourceId>,
        native: Option<NodeId>,
        pixelated: bool,
    },
    Text {
        index: usize,
        node: NodeId,
    },
}

impl DrawCommand {
    fn gradient(&self) -> Option<&LinearGradientDraw> {
        match self {
            Self::Quads { gradient, .. } => gradient.as_ref(),
            Self::BeginOpacityGroup { .. }
            | Self::EndOpacityGroup { .. }
            | Self::Text { .. }
            | Self::BackdropBlur { .. } => None,
        }
    }
}

pub(crate) fn linear_gradient_draw(
    positioning_rect: LayoutRect,
    angle_degrees: f32,
    repeating: bool,
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let angle = angle_degrees.to_radians();
    let direction = [angle.sin(), -angle.cos()];
    let center = [
        positioning_rect.x + positioning_rect.width * 0.5,
        positioning_rect.y + positioning_rect.height * 0.5,
    ];
    let half_length = direction[0].abs() * positioning_rect.width * 0.5
        + direction[1].abs() * positioning_rect.height * 0.5;
    let line_length = (half_length * 2.0).max(0.0001);
    LinearGradientDraw {
        start_end: [
            center[0] - direction[0] * half_length,
            center[1] - direction[1] * half_length,
            center[0] + direction[0] * half_length,
            center[1] + direction[1] * half_length,
        ],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| {
                    position.fraction + position.length / line_length
                }),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: u32::from(repeating),
        geometry_flags: 0,
    }
}

pub(crate) fn radial_gradient_draw(
    positioning_rect: LayoutRect,
    shape: whisker_protocol::RadialGradientShape,
    extent: whisker_protocol::RadialGradientExtent,
    center: whisker_protocol::PaintPosition,
    explicit_radii: Option<(
        whisker_protocol::PaintLengthPercentage,
        whisker_protocol::PaintLengthPercentage,
    )>,
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let radii = radial_gradient_radii(positioning_rect, center, shape, extent, explicit_radii);
    let center = [
        positioning_rect.x + center.x.length + center.x.fraction * positioning_rect.width,
        positioning_rect.y + center.y.length + center.y.fraction * positioning_rect.height,
    ];
    let line_length = radii[0].max(0.0001);
    LinearGradientDraw {
        start_end: [center[0], center[1], radii[0], radii[1]],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| {
                    position.fraction + position.length / line_length
                }),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: 2,
        geometry_flags: 0,
    }
}

fn radial_gradient_radii(
    bounds: LayoutRect,
    center: whisker_protocol::PaintPosition,
    shape: whisker_protocol::RadialGradientShape,
    extent: whisker_protocol::RadialGradientExtent,
    explicit: Option<(
        whisker_protocol::PaintLengthPercentage,
        whisker_protocol::PaintLengthPercentage,
    )>,
) -> [f32; 2] {
    use whisker_protocol::{RadialGradientExtent, RadialGradientShape};

    let center_x = center.x.length + center.x.fraction * bounds.width;
    let center_y = center.y.length + center.y.fraction * bounds.height;
    if extent == RadialGradientExtent::Explicit {
        let (x, y) = explicit.expect("validated explicit radial gradient has radii");
        let radius_x = x.length + x.fraction * bounds.width;
        let radius_y = if shape == RadialGradientShape::Circle {
            radius_x
        } else {
            y.length + y.fraction * bounds.height
        };
        return [radius_x, radius_y];
    }

    let near_x = center_x.min(bounds.width - center_x).max(0.0);
    let far_x = center_x.max(bounds.width - center_x).max(0.0);
    let near_y = center_y.min(bounds.height - center_y).max(0.0);
    let far_y = center_y.max(bounds.height - center_y).max(0.0);
    let (x, y, corner) = match extent {
        RadialGradientExtent::ClosestSide => (near_x, near_y, false),
        RadialGradientExtent::FarthestSide => (far_x, far_y, false),
        RadialGradientExtent::ClosestCorner => (near_x, near_y, true),
        RadialGradientExtent::FarthestCorner => (far_x, far_y, true),
        RadialGradientExtent::Explicit => unreachable!(),
    };
    if shape == RadialGradientShape::Circle {
        let radius = if corner {
            x.hypot(y)
        } else if extent == RadialGradientExtent::ClosestSide {
            x.min(y)
        } else {
            x.max(y)
        };
        [radius, radius]
    } else {
        let scale = if corner {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };
        [x * scale, y * scale]
    }
}

pub(crate) fn conic_gradient_draw(
    positioning_rect: LayoutRect,
    from_degrees: f32,
    center: whisker_protocol::PaintPosition,
    stops: &[GradientStop],
    opacity: f32,
) -> LinearGradientDraw {
    let center = [
        positioning_rect.x + center.x.length + center.x.fraction * positioning_rect.width,
        positioning_rect.y + center.y.length + center.y.fraction * positioning_rect.height,
    ];
    LinearGradientDraw {
        start_end: [center[0], center[1], from_degrees / 360.0, 0.0],
        tile_rect: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        tile_stride: [positioning_rect.width, positioning_rect.height, 0.0, 0.0],
        tile_domain: [
            positioning_rect.x,
            positioning_rect.y,
            positioning_rect.width,
            positioning_rect.height,
        ],
        stops: stops
            .iter()
            .map(|stop| LinearGradientStopGpu {
                position: stop.position.map_or(0.0, |position| position.fraction),
                padding: [0.0; 3],
                color: gpu_color(&stop.color, opacity),
            })
            .collect(),
        kind: 3,
        geometry_flags: 0,
    }
}

#[cfg(test)]
mod tests;
