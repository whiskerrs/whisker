use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glyphon::{
    Cache, Color as TextColor, Resolution, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;
use wgpu::{
    BlendState, Buffer, BufferUsages, ColorTargetState, ColorWrites, CommandEncoderDescriptor,
    CompositeAlphaMode, Device, DeviceDescriptor, FragmentState, Instance, InstanceDescriptor,
    LoadOp, MultisampleState, Operations, PipelineCompilationOptions, PresentMode, PrimitiveState,
    Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp,
    Surface, SurfaceConfiguration, SurfaceError, TextureFormat, TextureUsages,
    TextureViewDescriptor, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};
use whisker_protocol::{
    BorderLineStyle, BoxPaint, LayoutRect, NodeId, PaintColor, PaintCorners, PaintLengthPercentage,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::scene::{LogicalClip, PaintCommand, is_transparent};
use crate::text::NativeTextHost;

const BOX_SHADER: &str = r#"
struct Viewport {
    logical_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: Viewport;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) outer_rect: vec4<f32>,
    @location(3) outer_radii_x: vec4<f32>,
    @location(4) outer_radii_y: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) inner_radii_x: vec4<f32>,
    @location(7) inner_radii_y: vec4<f32>,
    @location(8) border_widths: vec4<f32>,
    @location(9) mode: f32,
    @location(10) border_top_color: vec4<f32>,
    @location(11) border_right_color: vec4<f32>,
    @location(12) border_bottom_color: vec4<f32>,
    @location(13) border_left_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) logical_position: vec2<f32>,
    @location(2) outer_rect: vec4<f32>,
    @location(3) outer_radii_x: vec4<f32>,
    @location(4) outer_radii_y: vec4<f32>,
    @location(5) inner_rect: vec4<f32>,
    @location(6) inner_radii_x: vec4<f32>,
    @location(7) inner_radii_y: vec4<f32>,
    @location(8) border_widths: vec4<f32>,
    @location(9) @interpolate(flat) mode: f32,
    @location(10) border_top_color: vec4<f32>,
    @location(11) border_right_color: vec4<f32>,
    @location(12) border_bottom_color: vec4<f32>,
    @location(13) border_left_color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let normalized = input.position / viewport.logical_size;
    var output: VertexOutput;
    output.position = vec4<f32>(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0, 0.0, 1.0);
    output.color = input.color;
    output.logical_position = input.position;
    output.outer_rect = input.outer_rect;
    output.outer_radii_x = input.outer_radii_x;
    output.outer_radii_y = input.outer_radii_y;
    output.inner_rect = input.inner_rect;
    output.inner_radii_x = input.inner_radii_x;
    output.inner_radii_y = input.inner_radii_y;
    output.border_widths = input.border_widths;
    output.mode = input.mode;
    output.border_top_color = input.border_top_color;
    output.border_right_color = input.border_right_color;
    output.border_bottom_color = input.border_bottom_color;
    output.border_left_color = input.border_left_color;
    return output;
}

fn ellipse_distance(
    position: vec2<f32>,
    center: vec2<f32>,
    radius: vec2<f32>,
) -> f32 {
    let normalized = (position - center) / radius;
    return (length(normalized) - 1.0) * min(radius.x, radius.y);
}

fn rounded_rect_distance(
    position: vec2<f32>,
    rect: vec4<f32>,
    radii_x: vec4<f32>,
    radii_y: vec4<f32>,
) -> f32 {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    let outside = max(vec2<f32>(left, top) - position, position - vec2<f32>(right, bottom));
    var distance = max(outside.x, outside.y);

    let top_left = vec2<f32>(radii_x.x, radii_y.x);
    if top_left.x > 0.0 && top_left.y > 0.0
        && position.x < left + top_left.x && position.y < top + top_left.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(left + top_left.x, top + top_left.y),
            top_left,
        ));
    }

    let top_right = vec2<f32>(radii_x.y, radii_y.y);
    if top_right.x > 0.0 && top_right.y > 0.0
        && position.x > right - top_right.x && position.y < top + top_right.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(right - top_right.x, top + top_right.y),
            top_right,
        ));
    }

    let bottom_right = vec2<f32>(radii_x.z, radii_y.z);
    if bottom_right.x > 0.0 && bottom_right.y > 0.0
        && position.x > right - bottom_right.x && position.y > bottom - bottom_right.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(right - bottom_right.x, bottom - bottom_right.y),
            bottom_right,
        ));
    }

    let bottom_left = vec2<f32>(radii_x.w, radii_y.w);
    if bottom_left.x > 0.0 && bottom_left.y > 0.0
        && position.x < left + bottom_left.x && position.y > bottom - bottom_left.y {
        distance = max(distance, ellipse_distance(
            position,
            vec2<f32>(left + bottom_left.x, bottom - bottom_left.y),
            bottom_left,
        ));
    }

    return distance;
}

fn shape_coverage(distance: f32) -> f32 {
    let smoothing = max(fwidth(distance), 0.0001);
    return clamp(0.5 - distance / smoothing, 0.0, 1.0);
}

fn border_side(position: vec2<f32>, rect: vec4<f32>, widths: vec4<f32>) -> f32 {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    var selected = -1.0;
    var score = 1e20;

    if widths.x > 0.0 {
        selected = 0.0;
        score = (position.y - top) / widths.x;
    }
    if widths.y > 0.0 {
        let candidate = (right - position.x) / widths.y;
        if candidate < score {
            selected = 1.0;
            score = candidate;
        }
    }
    if widths.z > 0.0 {
        let candidate = (bottom - position.y) / widths.z;
        if candidate < score {
            selected = 2.0;
            score = candidate;
        }
    }
    if widths.w > 0.0 {
        let candidate = (position.x - left) / widths.w;
        if candidate < score {
            selected = 3.0;
        }
    }
    return selected;
}

fn border_color(input: VertexOutput, side: f32) -> vec4<f32> {
    if side < 0.5 {
        return input.border_top_color;
    }
    if side < 1.5 {
        return input.border_right_color;
    }
    if side < 2.5 {
        return input.border_bottom_color;
    }
    return input.border_left_color;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let outer_distance = rounded_rect_distance(
        input.logical_position,
        input.outer_rect,
        input.outer_radii_x,
        input.outer_radii_y,
    );
    let outer_coverage = shape_coverage(outer_distance);
    if input.mode < 0.0 {
        return vec4<f32>(input.color.rgb, input.color.a * outer_coverage);
    }

    var inner_coverage = 0.0;
    if input.inner_rect.z > 0.0 && input.inner_rect.w > 0.0 {
        let inner_distance = rounded_rect_distance(
            input.logical_position,
            input.inner_rect,
            input.inner_radii_x,
            input.inner_radii_y,
        );
        inner_coverage = shape_coverage(inner_distance);
    }
    let side = border_side(input.logical_position, input.outer_rect, input.border_widths);
    let color = border_color(input, side);
    let coverage = max(outer_coverage - inner_coverage, 0.0);
    return vec4<f32>(color.rgb, color.a * coverage);
}
"#;

#[derive(Debug)]
pub(crate) struct GpuError(String);

impl fmt::Display for GpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for GpuError {}

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
}

impl BoxVertex {
    const ATTRIBUTES: [VertexAttribute; 14] = [
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
    padding: [f32; 2],
}

enum DrawCommand {
    Quads {
        vertices: Range<u32>,
        clip: LogicalClip,
    },
    Text {
        index: usize,
        node: NodeId,
    },
}

pub(crate) struct GpuRenderer {
    surface: Surface<'static>,
    device: Device,
    queue: Queue,
    config: SurfaceConfiguration,
    box_pipeline: RenderPipeline,
    viewport_buffer: Buffer,
    viewport_bind_group: wgpu::BindGroup,
    text_viewport: Viewport,
    text_atlas: TextAtlas,
    text_renderers: HashMap<NodeId, TextRenderer>,
}

impl GpuRenderer {
    pub(crate) async fn new(window: Arc<Window>) -> Result<Self, GpuError> {
        let size = window.inner_size();
        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance
            .create_surface(window)
            .map_err(|error| GpuError(format!("create Metal surface: {error}")))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..RequestAdapterOptions::default()
            })
            .await
            .map_err(|error| GpuError(format!("select Metal adapter: {error}")))?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default())
            .await
            .map_err(|error| GpuError(format!("create Metal device: {error}")))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| GpuError("Metal surface exposes no texture format".into()))?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == PresentMode::Fifo)
            .unwrap_or(PresentMode::AutoVsync);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let viewport_uniform = ViewportUniform {
            logical_size: [1.0, 1.0],
            padding: [0.0; 2],
        };
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker macOS logical viewport"),
            contents: bytemuck::bytes_of(&viewport_uniform),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker macOS viewport layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker macOS viewport"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker macOS box shader"),
            source: ShaderSource::Wgsl(BOX_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker macOS box pipeline layout"),
            bind_group_layouts: &[&viewport_layout],
            push_constant_ranges: &[],
        });
        let box_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("whisker macOS box pipeline"),
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

        let text_cache = Cache::new(&device);
        let text_viewport = Viewport::new(&device, &text_cache);
        let text_atlas = TextAtlas::new(&device, &queue, &text_cache, format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            box_pipeline,
            viewport_buffer,
            viewport_bind_group,
            text_viewport,
            text_atlas,
            text_renderers: HashMap::new(),
        })
    }

    pub(crate) fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub(crate) fn render(
        &mut self,
        commands: &[PaintCommand<'_>],
        text: &mut NativeTextHost,
        logical_size: [f32; 2],
        scale: f32,
    ) -> Result<(), GpuError> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }
        self.queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                logical_size,
                padding: [0.0; 2],
            }),
        );
        self.text_viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );

        let mut vertices = Vec::new();
        let mut draws = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            match command {
                PaintCommand::Box {
                    rect,
                    paint,
                    clip,
                    opacity,
                } => {
                    let start = vertices.len() as u32;
                    lower_box(*rect, paint, *opacity, &mut vertices);
                    let end = vertices.len() as u32;
                    if start != end {
                        draws.push(DrawCommand::Quads {
                            vertices: start..end,
                            clip: *clip,
                        });
                    }
                }
                PaintCommand::Text { node, .. } => {
                    draws.push(DrawCommand::Text { index, node: *node })
                }
            }
        }
        let live_text_nodes = draws
            .iter()
            .filter_map(|draw| match draw {
                DrawCommand::Text { node, .. } => Some(*node),
                DrawCommand::Quads { .. } => None,
            })
            .collect::<HashSet<_>>();
        self.text_renderers
            .retain(|node, _| live_text_nodes.contains(node));
        let mut prepared_text_nodes = HashSet::new();
        for draw in &draws {
            let DrawCommand::Text { index, node } = draw else {
                continue;
            };
            let PaintCommand::Text {
                rect,
                content,
                clip,
                opacity,
                ..
            } = &commands[*index]
            else {
                unreachable!();
            };
            let Some(id) = content.prepared_content else {
                continue;
            };
            let Some(prepared) = text.prepared.get(&id) else {
                continue;
            };
            if !self.text_renderers.contains_key(node) {
                self.text_renderers.insert(
                    *node,
                    TextRenderer::new(
                        &mut self.text_atlas,
                        &self.device,
                        MultisampleState::default(),
                        None,
                    ),
                );
            }
            let bounds = text_bounds(*clip, self.config.width, self.config.height, scale);
            let color = text_color(&content.paint.foreground, *opacity);
            self.text_renderers
                .get_mut(node)
                .expect("renderer inserted for live text node")
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut text.font_system,
                    &mut self.text_atlas,
                    &self.text_viewport,
                    [TextArea {
                        buffer: &prepared.buffer,
                        left: rect.x * scale,
                        top: rect.y * scale,
                        scale,
                        bounds,
                        default_color: color,
                        custom_glyphs: &[],
                    }],
                    &mut text.swash_cache,
                )
                .map_err(|error| GpuError(format!("prepare glyph atlas: {error}")))?;
            prepared_text_nodes.insert(*node);
        }
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("whisker macOS box vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                })
        });

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture().map_err(|error| {
                    GpuError(format!("acquire Metal frame after recovery: {error}"))
                })?
            }
            Err(SurfaceError::Timeout) => return Ok(()),
            Err(error) => return Err(GpuError(format!("acquire Metal frame: {error}"))),
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("whisker macOS frame"),
            });
        {
            let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker macOS clear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        for draw in draws {
            match draw {
                DrawCommand::Quads { vertices, clip } => {
                    let Some((x, y, width, height)) = self.scissor(clip, scale) else {
                        continue;
                    };
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker macOS boxes"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_scissor_rect(x, y, width, height);
                    pass.set_pipeline(&self.box_pipeline);
                    pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                    pass.set_vertex_buffer(
                        0,
                        vertex_buffer
                            .as_ref()
                            .expect("quad draw has vertices")
                            .slice(..),
                    );
                    pass.draw(vertices, 0..1);
                }
                DrawCommand::Text { node, .. } => {
                    if !prepared_text_nodes.contains(&node) {
                        continue;
                    }
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("whisker macOS text"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    self.text_renderers
                        .get(&node)
                        .expect("prepared text renderer remains retained")
                        .render(&self.text_atlas, &self.text_viewport, &mut pass)
                        .map_err(|error| GpuError(format!("encode glyph draw: {error}")))?;
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.text_atlas.trim();
        Ok(())
    }

    fn scissor(&self, clip: LogicalClip, scale: f32) -> Option<(u32, u32, u32, u32)> {
        let left = (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let top = (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as u32;
        let right = (clip.right.unwrap_or(self.config.width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.width as f32) as u32;
        let bottom = (clip.bottom.unwrap_or(self.config.height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, self.config.height as f32) as u32;
        (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
    }
}

fn lower_box(rect: LayoutRect, paint: &BoxPaint, opacity: f32, vertices: &mut Vec<BoxVertex>) {
    let geometry = resolve_box_geometry(rect, paint);
    if !is_transparent(&paint.background_color) {
        push_quad(
            vertices,
            geometry,
            linear_color(&paint.background_color, opacity),
            -1.0,
            [[0.0; 4]; 4],
        );
    }
    let [top, right, bottom, left] = geometry.border_widths;
    let border_colors = [
        border_color(
            paint.border_styles.top,
            top,
            &paint.border_colors.top,
            opacity,
        ),
        border_color(
            paint.border_styles.right,
            right,
            &paint.border_colors.right,
            opacity,
        ),
        border_color(
            paint.border_styles.bottom,
            bottom,
            &paint.border_colors.bottom,
            opacity,
        ),
        border_color(
            paint.border_styles.left,
            left,
            &paint.border_colors.left,
            opacity,
        ),
    ];
    if border_colors.iter().any(|color| color[3] > 0.0) {
        push_quad(vertices, geometry, [0.0; 4], 1.0, border_colors);
    }
}

fn paints_line(style: BorderLineStyle) -> bool {
    !matches!(style, BorderLineStyle::None | BorderLineStyle::Hidden)
}

fn border_color(style: BorderLineStyle, width: f32, color: &PaintColor, opacity: f32) -> [f32; 4] {
    if paints_line(style) && width > 0.0 {
        linear_color(color, opacity)
    } else {
        [0.0; 4]
    }
}

fn resolve_length(value: PaintLengthPercentage, axis: f32) -> f32 {
    value.length + value.fraction * axis
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResolvedRadii {
    horizontal: [f32; 4],
    vertical: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BoxGeometry {
    outer_rect: LayoutRect,
    outer_radii: ResolvedRadii,
    inner_rect: LayoutRect,
    inner_radii: ResolvedRadii,
    border_widths: [f32; 4],
}

fn resolve_box_geometry(rect: LayoutRect, paint: &BoxPaint) -> BoxGeometry {
    let outer_radii = resolve_radii(&paint.border_radii, rect);
    let top = resolve_length(paint.border_widths.top, rect.height).min(rect.height);
    let right = resolve_length(paint.border_widths.right, rect.width).min(rect.width);
    let bottom = resolve_length(paint.border_widths.bottom, rect.height).min(rect.height);
    let left = resolve_length(paint.border_widths.left, rect.width).min(rect.width);
    let inner_rect = LayoutRect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    };
    let inner_radii = ResolvedRadii {
        horizontal: [
            (outer_radii.horizontal[0] - left).max(0.0),
            (outer_radii.horizontal[1] - right).max(0.0),
            (outer_radii.horizontal[2] - right).max(0.0),
            (outer_radii.horizontal[3] - left).max(0.0),
        ],
        vertical: [
            (outer_radii.vertical[0] - top).max(0.0),
            (outer_radii.vertical[1] - top).max(0.0),
            (outer_radii.vertical[2] - bottom).max(0.0),
            (outer_radii.vertical[3] - bottom).max(0.0),
        ],
    };
    BoxGeometry {
        outer_rect: rect,
        outer_radii,
        inner_rect,
        inner_radii,
        border_widths: [top, right, bottom, left],
    }
}

fn resolve_radii(radii: &PaintCorners<PaintLengthPercentage>, rect: LayoutRect) -> ResolvedRadii {
    let values = [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ];
    let mut horizontal = values.map(|radius| resolve_length(radius, rect.width));
    let mut vertical = values.map(|radius| resolve_length(radius, rect.height));
    let ratios = [
        ratio(rect.width, horizontal[0] + horizontal[1]),
        ratio(rect.width, horizontal[3] + horizontal[2]),
        ratio(rect.height, vertical[0] + vertical[3]),
        ratio(rect.height, vertical[1] + vertical[2]),
    ];
    let scale = ratios.into_iter().fold(1.0_f32, f32::min);
    for radius in &mut horizontal {
        *radius *= scale;
    }
    for radius in &mut vertical {
        *radius *= scale;
    }
    ResolvedRadii {
        horizontal,
        vertical,
    }
}

fn ratio(available: f32, required: f32) -> f32 {
    if required > available && required > 0.0 {
        available / required
    } else {
        1.0
    }
}

fn push_quad(
    vertices: &mut Vec<BoxVertex>,
    geometry: BoxGeometry,
    color: [f32; 4],
    mode: f32,
    border_colors: [[f32; 4]; 4],
) {
    let rect = geometry.outer_rect;
    if rect.width <= 0.0
        || rect.height <= 0.0
        || (color[3] <= 0.0 && border_colors.iter().all(|color| color[3] <= 0.0))
    {
        return;
    }
    let left = rect.x;
    let top = rect.y;
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    for position in [
        [left, top],
        [right, top],
        [left, bottom],
        [left, bottom],
        [right, top],
        [right, bottom],
    ] {
        vertices.push(BoxVertex {
            position,
            color,
            outer_rect: [rect.x, rect.y, rect.width, rect.height],
            outer_radii_x: geometry.outer_radii.horizontal,
            outer_radii_y: geometry.outer_radii.vertical,
            inner_rect: [
                geometry.inner_rect.x,
                geometry.inner_rect.y,
                geometry.inner_rect.width,
                geometry.inner_rect.height,
            ],
            inner_radii_x: geometry.inner_radii.horizontal,
            inner_radii_y: geometry.inner_radii.vertical,
            border_widths: geometry.border_widths,
            mode,
            border_colors,
        });
    }
}

fn text_bounds(clip: LogicalClip, width: u32, height: u32, scale: f32) -> TextBounds {
    TextBounds {
        left: (clip.left.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        top: (clip.top.unwrap_or(0.0) * scale).floor().max(0.0) as i32,
        right: (clip.right.unwrap_or(width as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, width as f32) as i32,
        bottom: (clip.bottom.unwrap_or(height as f32 / scale) * scale)
            .ceil()
            .clamp(0.0, height as f32) as i32,
    }
}

fn text_color(color: &PaintColor, opacity: f32) -> TextColor {
    let [red, green, blue, alpha] = srgba(color, opacity);
    TextColor::rgba(
        (red * 255.0).round() as u8,
        (green * 255.0).round() as u8,
        (blue * 255.0).round() as u8,
        (alpha * 255.0).round() as u8,
    )
}

fn linear_color(color: &PaintColor, opacity: f32) -> [f32; 4] {
    let [red, green, blue, alpha] = srgba(color, opacity);
    [
        srgb_to_linear(red),
        srgb_to_linear(green),
        srgb_to_linear(blue),
        alpha,
    ]
}

fn srgba(color: &PaintColor, opacity: f32) -> [f32; 4] {
    let mut color = match color {
        PaintColor::Named(name) => csscolorparser::parse(name)
            .map(|color| color.to_array())
            .unwrap_or([0.0, 0.0, 0.0, 0.0]),
        PaintColor::Srgba {
            red,
            green,
            blue,
            alpha,
        } => [
            *red as f32 / 255.0,
            *green as f32 / 255.0,
            *blue as f32 / 255.0,
            *alpha,
        ],
        PaintColor::Hsla {
            hue_degrees,
            saturation,
            lightness,
            alpha,
        } => hsl_to_srgba(
            *hue_degrees,
            *saturation / 100.0,
            *lightness / 100.0,
            *alpha,
        ),
    };
    color[3] *= opacity;
    color
}

fn hsl_to_srgba(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> [f32; 4] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let match_value = lightness - chroma / 2.0;
    [
        red + match_value,
        green + match_value,
        blue + match_value,
        alpha,
    ]
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_protocol::{PaintCorners, PaintEdges};

    fn paint(background_color: PaintColor) -> BoxPaint {
        let zero = PaintLengthPercentage::default();
        BoxPaint {
            background_color,
            border_widths: PaintEdges {
                top: PaintLengthPercentage {
                    length: 1.0,
                    fraction: 0.0,
                },
                right: zero,
                bottom: zero,
                left: zero,
            },
            border_colors: PaintEdges {
                top: PaintColor::Named("blue".into()),
                right: PaintColor::default(),
                bottom: PaintColor::default(),
                left: PaintColor::default(),
            },
            border_styles: PaintEdges {
                top: BorderLineStyle::Solid,
                right: BorderLineStyle::None,
                bottom: BorderLineStyle::Hidden,
                left: BorderLineStyle::None,
            },
            border_radii: PaintCorners {
                top_left: zero,
                top_right: zero,
                bottom_right: zero,
                bottom_left: zero,
            },
        }
    }

    #[test]
    fn box_lowering_emits_background_and_visible_borders() {
        let mut vertices = Vec::new();
        lower_box(
            LayoutRect {
                x: 2.0,
                y: 3.0,
                width: 20.0,
                height: 10.0,
            },
            &paint(PaintColor::Named("red".into())),
            0.5,
            &mut vertices,
        );
        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[0].position, [2.0, 3.0]);
        assert_eq!(vertices[0].outer_rect, [2.0, 3.0, 20.0, 10.0]);
        assert_eq!(vertices[0].outer_radii_x, [0.0; 4]);
        assert_eq!(vertices[0].outer_radii_y, [0.0; 4]);
        assert_eq!(vertices[0].mode, -1.0);
        assert!((vertices[0].color[3] - 0.5).abs() < f32::EPSILON);
        assert_eq!(vertices[6].position, [2.0, 3.0]);
        assert_eq!(vertices[8].position, [2.0, 13.0]);
        assert_eq!(vertices[6].inner_rect, [2.0, 4.0, 20.0, 9.0]);
        assert_eq!(vertices[6].border_widths, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].mode, 1.0);
        assert!(vertices[6].border_colors[0][2] > 0.99);
        assert_eq!(vertices[6].border_colors[1], [0.0; 4]);

        vertices.clear();
        let mut transparent = paint(PaintColor::Named("transparent".into()));
        transparent.border_styles.top = BorderLineStyle::None;
        lower_box(LayoutRect::default(), &transparent, 1.0, &mut vertices);
        assert!(vertices.is_empty());
    }

    #[test]
    fn rounded_radii_resolve_percentages_and_scale_overlaps() {
        let resolved = resolve_radii(
            &PaintCorners {
                top_left: PaintLengthPercentage {
                    length: 30.0,
                    fraction: 0.0,
                },
                top_right: PaintLengthPercentage {
                    length: 0.0,
                    fraction: 0.5,
                },
                bottom_right: PaintLengthPercentage {
                    length: 30.0,
                    fraction: 0.0,
                },
                bottom_left: PaintLengthPercentage {
                    length: 30.0,
                    fraction: 0.0,
                },
            },
            LayoutRect {
                x: 5.0,
                y: 6.0,
                width: 100.0,
                height: 40.0,
            },
        );
        assert!((resolved.horizontal[0] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.horizontal[1] - 100.0 / 3.0).abs() < 0.001);
        assert!((resolved.horizontal[2] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.horizontal[3] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[0] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[1] - 40.0 / 3.0).abs() < 0.001);
        assert!((resolved.vertical[2] - 20.0).abs() < f32::EPSILON);
        assert!((resolved.vertical[3] - 20.0).abs() < f32::EPSILON);

        let mut rounded = paint(PaintColor::Named("red".into()));
        rounded.border_radii = PaintCorners {
            top_left: PaintLengthPercentage {
                length: 8.0,
                fraction: 0.0,
            },
            top_right: PaintLengthPercentage::default(),
            bottom_right: PaintLengthPercentage::default(),
            bottom_left: PaintLengthPercentage::default(),
        };
        let mut vertices = Vec::new();
        lower_box(
            LayoutRect {
                x: 1.0,
                y: 2.0,
                width: 20.0,
                height: 10.0,
            },
            &rounded,
            1.0,
            &mut vertices,
        );
        assert_eq!(vertices[0].outer_radii_x, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[0].outer_radii_y, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].inner_radii_x, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[6].inner_radii_y, [7.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            resolve_radii(&rounded.border_radii, LayoutRect::default()),
            ResolvedRadii {
                horizontal: [0.0; 4],
                vertical: [0.0; 4],
            }
        );
    }

    #[test]
    fn rounded_border_geometry_preserves_corner_arc_centers() {
        let three = PaintLengthPercentage {
            length: 3.0,
            fraction: 0.0,
        };
        let mut bordered = paint(PaintColor::Named("green".into()));
        bordered.border_widths = PaintEdges {
            top: three,
            right: three,
            bottom: three,
            left: three,
        };
        bordered.border_styles = PaintEdges {
            top: BorderLineStyle::Solid,
            right: BorderLineStyle::Solid,
            bottom: BorderLineStyle::Solid,
            left: BorderLineStyle::Solid,
        };
        bordered.border_colors = PaintEdges {
            top: PaintColor::Named("yellow".into()),
            right: PaintColor::Named("yellow".into()),
            bottom: PaintColor::Named("yellow".into()),
            left: PaintColor::Named("yellow".into()),
        };
        bordered.border_radii = PaintCorners {
            top_left: PaintLengthPercentage {
                length: 40.0,
                fraction: 0.0,
            },
            top_right: PaintLengthPercentage {
                length: 8.0,
                fraction: 0.0,
            },
            bottom_right: PaintLengthPercentage {
                length: 40.0,
                fraction: 0.0,
            },
            bottom_left: PaintLengthPercentage {
                length: 8.0,
                fraction: 0.0,
            },
        };
        let geometry = resolve_box_geometry(
            LayoutRect {
                x: 10.0,
                y: 20.0,
                width: 200.0,
                height: 88.0,
            },
            &bordered,
        );

        assert_eq!(geometry.inner_rect.x, 13.0);
        assert_eq!(geometry.inner_rect.y, 23.0);
        assert_eq!(geometry.inner_rect.width, 194.0);
        assert_eq!(geometry.inner_rect.height, 82.0);
        assert_eq!(geometry.outer_radii.horizontal, [40.0, 8.0, 40.0, 8.0]);
        assert_eq!(geometry.outer_radii.vertical, [40.0, 8.0, 40.0, 8.0]);
        assert_eq!(geometry.inner_radii.horizontal, [37.0, 5.0, 37.0, 5.0]);
        assert_eq!(geometry.inner_radii.vertical, [37.0, 5.0, 37.0, 5.0]);
        assert_eq!(
            geometry.outer_rect.x + geometry.outer_radii.horizontal[0],
            geometry.inner_rect.x + geometry.inner_radii.horizontal[0]
        );
        assert_eq!(
            geometry.outer_rect.y + geometry.outer_radii.vertical[0],
            geometry.inner_rect.y + geometry.inner_radii.vertical[0]
        );

        let mut vertices = Vec::new();
        lower_box(geometry.outer_rect, &bordered, 1.0, &mut vertices);
        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[6].mode, 1.0);
        assert!(
            vertices[6]
                .border_colors
                .windows(2)
                .all(|colors| colors[0] == colors[1])
        );
    }

    #[test]
    fn protocol_colors_convert_to_gpu_and_glyph_colors() {
        let hsl = PaintColor::Hsla {
            hue_degrees: 120.0,
            saturation: 100.0,
            lightness: 50.0,
            alpha: 0.8,
        };
        let [red, green, blue, alpha] = srgba(&hsl, 0.5);
        assert!(red.abs() < f32::EPSILON);
        assert!((green - 1.0).abs() < f32::EPSILON);
        assert!(blue.abs() < f32::EPSILON);
        assert!((alpha - 0.4).abs() < f32::EPSILON);
        assert_eq!(text_color(&hsl, 0.5), TextColor::rgba(0, 255, 0, 102));

        assert_eq!(
            srgba(&PaintColor::Named("not-a-css-color".into()), 1.0),
            [0.0; 4]
        );
        assert!((srgb_to_linear(0.02) - 0.02 / 12.92).abs() < f32::EPSILON);
        assert!(srgb_to_linear(1.0) > 0.99);
    }

    #[test]
    fn text_bounds_scale_and_clamp_logical_clips() {
        let bounds = text_bounds(
            LogicalClip {
                left: Some(-2.0),
                top: Some(3.0),
                right: Some(70.0),
                bottom: None,
            },
            100,
            80,
            2.0,
        );
        assert_eq!(
            bounds,
            TextBounds {
                left: 0,
                top: 6,
                right: 100,
                bottom: 80
            }
        );
    }
}
