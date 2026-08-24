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
use whisker_protocol::{NodeId, Transform};

use crate::paint::box_paint::{BoxPrimitive, lower_box};
use crate::paint::color::text_color;
use crate::scene::{LogicalClip, PaintCommand};
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
    @location(14) border_styles: vec4<f32>,
    @location(15) transformed_position: vec4<f32>,
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
    @location(14) @interpolate(flat) border_styles: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let transformed = input.transformed_position;
    var output: VertexOutput;
    output.position = vec4<f32>(
        transformed.x / viewport.logical_size.x * 2.0 - transformed.w,
        transformed.w - transformed.y / viewport.logical_size.y * 2.0,
        transformed.z,
        transformed.w,
    );
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
    output.border_styles = input.border_styles;
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

fn border_side(position: vec2<f32>, rect: vec4<f32>, widths: vec4<f32>) -> vec2<f32> {
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
            score = candidate;
        }
    }
    return vec2<f32>(selected, clamp(score, 0.0, 1.0));
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

fn border_style(input: VertexOutput, side: f32) -> f32 {
    if side < 0.5 {
        return input.border_styles.x;
    }
    if side < 1.5 {
        return input.border_styles.y;
    }
    if side < 2.5 {
        return input.border_styles.z;
    }
    return input.border_styles.w;
}

fn border_width(input: VertexOutput, side: f32) -> f32 {
    if side < 0.5 {
        return input.border_widths.x;
    }
    if side < 1.5 {
        return input.border_widths.y;
    }
    if side < 2.5 {
        return input.border_widths.z;
    }
    return input.border_widths.w;
}

fn border_path_position(position: vec2<f32>, rect: vec4<f32>, side: f32) -> f32 {
    let left = rect.x;
    let top = rect.y;
    let right = left + rect.z;
    let bottom = top + rect.w;
    if side < 0.5 {
        return position.x - left;
    }
    if side < 1.5 {
        return rect.z + position.y - top;
    }
    if side < 2.5 {
        return rect.z + rect.w + right - position.x;
    }
    return rect.z * 2.0 + rect.w + bottom - position.y;
}

fn patterned_coverage(
    style: f32,
    path_position: f32,
    width: f32,
    depth: f32,
) -> f32 {
    if style < 1.5 || style >= 3.5 {
        return 1.0;
    }
    if style < 2.5 {
        let period = max(width * 4.0, 0.0001);
        let phase = fract(path_position / period) * period;
        let distance = max(-phase, phase - width * 3.0);
        return shape_coverage(distance);
    }
    let period = max(width * 2.0, 0.0001);
    let along = abs(fract(path_position / period) * period - width);
    let across = abs(depth - 0.5) * width;
    let distance = length(vec2<f32>(along, across)) - width * 0.5;
    return shape_coverage(distance);
}

fn double_coverage(style: f32, depth: f32) -> f32 {
    if style < 3.5 || style >= 4.5 {
        return 1.0;
    }
    let distance = min(abs(depth - 1.0 / 6.0), abs(depth - 5.0 / 6.0)) - 1.0 / 6.0;
    return shape_coverage(distance);
}

fn shade(color: vec4<f32>, amount: f32) -> vec4<f32> {
    var rgb = color.rgb;
    if amount < 0.0 {
        rgb = rgb * (1.0 + amount);
    } else {
        rgb = rgb + (vec3<f32>(1.0) - rgb) * amount;
    }
    return vec4<f32>(rgb, color.a);
}

fn styled_color(color: vec4<f32>, style: f32, side: f32, depth: f32) -> vec4<f32> {
    let top_or_left = side < 0.5 || side > 2.5;
    let inset_amount = select(0.35, -0.35, top_or_left);
    if style >= 6.5 && style < 7.5 {
        return shade(color, inset_amount);
    }
    if style >= 7.5 {
        return shade(color, -inset_amount);
    }
    if style >= 4.5 && style < 5.5 {
        return shade(color, select(-inset_amount, inset_amount, depth < 0.5));
    }
    if style >= 5.5 && style < 6.5 {
        return shade(color, select(inset_amount, -inset_amount, depth < 0.5));
    }
    return color;
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
    let side_and_depth = border_side(
        input.logical_position,
        input.outer_rect,
        input.border_widths,
    );
    let side = side_and_depth.x;
    let depth = side_and_depth.y;
    let style = border_style(input, side);
    let width = border_width(input, side);
    let path_position = border_path_position(input.logical_position, input.outer_rect, side);
    let color = styled_color(border_color(input, side), style, side, depth);
    let style_coverage = patterned_coverage(style, path_position, width, depth)
        * double_coverage(style, depth);
    let coverage = max(outer_coverage - inner_coverage, 0.0) * style_coverage;
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
    padding: [f32; 2],
}

struct BoxGpuPipeline {
    pipeline: RenderPipeline,
    viewport_buffer: Buffer,
    viewport_bind_group: wgpu::BindGroup,
}

impl BoxGpuPipeline {
    fn new(device: &Device, format: TextureFormat) -> Self {
        let viewport_uniform = ViewportUniform {
            logical_size: [1.0, 1.0],
            padding: [0.0; 2],
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
            label: Some("whisker Desktop viewport"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("whisker Desktop box shader"),
            source: ShaderSource::Wgsl(BOX_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whisker Desktop box pipeline layout"),
            bind_group_layouts: &[&viewport_layout],
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
        }
    }

    fn update_viewport(&self, queue: &Queue, logical_size: [f32; 2]) {
        queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::bytes_of(&ViewportUniform {
                logical_size,
                padding: [0.0; 2],
            }),
        );
    }
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
    box_gpu: BoxGpuPipeline,
    text_viewport: Viewport,
    text_atlas: TextAtlas,
    text_renderers: HashMap<NodeId, TextRenderer>,
}

impl GpuRenderer {
    pub(crate) async fn new(
        target: impl Into<SurfaceTarget<'static>>,
        physical_size: [u32; 2],
    ) -> Result<Self, GpuError> {
        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance
            .create_surface(target)
            .map_err(|error| GpuError(format!("create Desktop GPU surface: {error}")))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..RequestAdapterOptions::default()
            })
            .await
            .map_err(|error| GpuError(format!("select Desktop GPU adapter: {error}")))?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor::default())
            .await
            .map_err(|error| GpuError(format!("create Desktop GPU device: {error}")))?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
            .ok_or_else(|| {
                GpuError(
                    "Desktop GPU surface exposes no non-sRGB format for CSS compositing".into(),
                )
            })?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == PresentMode::Fifo)
            .unwrap_or(PresentMode::AutoVsync);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: physical_size[0].max(1),
            height: physical_size[1].max(1),
            present_mode,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let box_gpu = BoxGpuPipeline::new(&device, format);

        let text_cache = Cache::new(&device);
        let text_viewport = Viewport::new(&device, &text_cache);
        let text_atlas = TextAtlas::new(&device, &queue, &text_cache, format);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            box_gpu,
            text_viewport,
            text_atlas,
            text_renderers: HashMap::new(),
        })
    }

    pub(crate) fn resize(&mut self, physical_size: [u32; 2]) {
        if physical_size[0] == 0 || physical_size[1] == 0 {
            return;
        }
        self.config.width = physical_size[0];
        self.config.height = physical_size[1];
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
        self.box_gpu.update_viewport(&self.queue, logical_size);
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
                    transform,
                    opacity,
                } => {
                    let start = vertices.len() as u32;
                    lower_box(*rect, paint, *opacity, |primitive| {
                        push_transformed_quad(&mut vertices, primitive, *transform);
                    });
                    let end = vertices.len() as u32;
                    if start != end {
                        draws.push(DrawCommand::Quads {
                            vertices: start..end,
                            clip: *clip,
                        });
                    }
                }
                PaintCommand::Text {
                    node, transform, ..
                } => {
                    // Glyph transforms are implemented with the SetText paint slice.
                    let _ = transform;
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
                    label: Some("whisker Desktop box vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: BufferUsages::VERTEX,
                })
        });

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(SurfaceError::Lost | SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture().map_err(|error| {
                    GpuError(format!("acquire Desktop GPU frame after recovery: {error}"))
                })?
            }
            Err(SurfaceError::Timeout) => return Ok(()),
            Err(error) => return Err(GpuError(format!("acquire Desktop GPU frame: {error}"))),
        };
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("whisker Desktop frame"),
            });
        {
            let _clear = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop clear"),
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
                        label: Some("whisker Desktop boxes"),
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
                    pass.set_pipeline(&self.box_gpu.pipeline);
                    pass.set_bind_group(0, &self.box_gpu.viewport_bind_group, &[]);
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
                        label: Some("whisker Desktop text"),
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

#[cfg(test)]
fn push_quad(vertices: &mut Vec<BoxVertex>, primitive: BoxPrimitive) {
    push_transformed_quad(vertices, primitive, Transform::IDENTITY);
}

fn push_transformed_quad(
    vertices: &mut Vec<BoxVertex>,
    primitive: BoxPrimitive,
    transform: Transform,
) {
    let rect = primitive.outer_rect;
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
            color: primitive.color,
            outer_rect: [rect.x, rect.y, rect.width, rect.height],
            outer_radii_x: primitive.outer_radii_x,
            outer_radii_y: primitive.outer_radii_y,
            inner_rect: [
                primitive.inner_rect.x,
                primitive.inner_rect.y,
                primitive.inner_rect.width,
                primitive.inner_rect.height,
            ],
            inner_radii_x: primitive.inner_radii_x,
            inner_radii_y: primitive.inner_radii_y,
            border_widths: primitive.border_widths,
            mode: primitive.kind.shader_mode(),
            border_colors: primitive.border_colors,
            border_styles: primitive.border_styles,
            transformed_position: transform_point(transform, position),
        });
    }
}

fn transform_point(transform: Transform, [x, y]: [f32; 2]) -> [f32; 4] {
    [
        transform.0[0] * x + transform.0[4] * y + transform.0[12],
        transform.0[1] * x + transform.0[5] * y + transform.0[13],
        transform.0[2] * x + transform.0[6] * y + transform.0[14],
        transform.0[3] * x + transform.0[7] * y + transform.0[15],
    ]
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

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_box_primitives_offscreen(
    primitives: &[BoxPrimitive],
    logical_size: [u32; 2],
) -> Result<Vec<u8>, GpuError> {
    let clipped = primitives
        .iter()
        .copied()
        .map(|primitive| (primitive, LogicalClip::default(), Transform::IDENTITY))
        .collect::<Vec<_>>();
    render_clipped_box_primitives_offscreen(&clipped, logical_size).await
}

#[cfg(all(test, feature = "host-conformance"))]
pub(crate) async fn render_clipped_box_primitives_offscreen(
    primitives: &[(BoxPrimitive, LogicalClip, Transform)],
    logical_size: [u32; 2],
) -> Result<Vec<u8>, GpuError> {
    let [width, height] = logical_size;
    if width == 0 || height == 0 {
        return Err(GpuError(
            "offscreen checkpoint has an empty viewport".into(),
        ));
    }

    let instance = Instance::new(&InstanceDescriptor::default());
    let adapter = instance
        .request_adapter(&RequestAdapterOptions::default())
        .await
        .map_err(|error| GpuError(format!("select offscreen Desktop GPU adapter: {error}")))?;
    let (device, queue) = adapter
        .request_device(&DeviceDescriptor::default())
        .await
        .map_err(|error| GpuError(format!("create offscreen Desktop GPU device: {error}")))?;
    let format = TextureFormat::Rgba8Unorm;
    let box_gpu = BoxGpuPipeline::new(&device, format);
    box_gpu.update_viewport(&queue, [width as f32, height as f32]);

    let mut vertices = Vec::new();
    let mut draws = Vec::new();
    for (primitive, clip, transform) in primitives {
        let start = vertices.len() as u32;
        push_transformed_quad(&mut vertices, *primitive, *transform);
        draws.push((start..vertices.len() as u32, *clip));
    }
    let vertex_buffer = (!vertices.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whisker Desktop conformance box vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: BufferUsages::VERTEX,
        })
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker Desktop conformance checkpoint"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let unpadded_bytes_per_row = width * 4;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("whisker Desktop conformance readback"),
        size: u64::from(padded_bytes_per_row) * u64::from(height),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("whisker Desktop conformance encoder"),
    });
    {
        encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("whisker Desktop conformance clear"),
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
    if let Some(vertex_buffer) = &vertex_buffer {
        for (range, clip) in draws {
            let Some((x, y, clip_width, clip_height)) = offscreen_scissor(clip, width, height)
            else {
                continue;
            };
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("whisker Desktop conformance boxes"),
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
            pass.set_scissor_rect(x, y, clip_width, clip_height);
            pass.set_pipeline(&box_gpu.pipeline);
            pass.set_bind_group(0, &box_gpu.viewport_bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(range, 0..1);
        }
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait())
        .map_err(|error| GpuError(format!("wait for Desktop GPU checkpoint: {error}")))?;
    receiver
        .recv()
        .map_err(|error| GpuError(format!("receive Desktop GPU checkpoint: {error}")))?
        .map_err(|error| GpuError(format!("map Desktop GPU checkpoint: {error}")))?;

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(padded_bytes_per_row as usize) {
        pixels.extend_from_slice(&row[..unpadded_bytes_per_row as usize]);
    }
    drop(mapped);
    readback.unmap();
    Ok(pixels)
}

#[cfg(all(test, feature = "host-conformance"))]
fn offscreen_scissor(clip: LogicalClip, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let left = clip.left.unwrap_or(0.0).floor().max(0.0) as u32;
    let top = clip.top.unwrap_or(0.0).floor().max(0.0) as u32;
    let right = clip
        .right
        .unwrap_or(width as f32)
        .ceil()
        .clamp(0.0, width as f32) as u32;
    let bottom = clip
        .bottom
        .unwrap_or(height as f32)
        .ceil()
        .clamp(0.0, height as f32) as u32;
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyphon::Color as TextColor;
    use whisker_protocol::{
        BorderLineStyle, BoxPaint, LayoutRect, PaintColor, PaintCornerRadius, PaintCorners,
        PaintEdges, PaintLengthPercentage,
    };

    use crate::paint::box_paint::{ResolvedRadii, resolve_box_geometry, resolve_radii};
    use crate::paint::color::srgba;

    fn radius(length: f32, fraction: f32) -> PaintCornerRadius {
        PaintCornerRadius::circular(PaintLengthPercentage { length, fraction })
    }

    fn lower_vertices(
        rect: LayoutRect,
        paint: &BoxPaint,
        opacity: f32,
        vertices: &mut Vec<BoxVertex>,
    ) {
        lower_box(rect, paint, opacity, |primitive| {
            push_quad(vertices, primitive);
        });
    }

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
                top_left: PaintCornerRadius::default(),
                top_right: PaintCornerRadius::default(),
                bottom_right: PaintCornerRadius::default(),
                bottom_left: PaintCornerRadius::default(),
            },
        }
    }

    #[test]
    fn box_lowering_emits_background_and_visible_borders() {
        let mut vertices = Vec::new();
        lower_vertices(
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
        lower_vertices(LayoutRect::default(), &transparent, 1.0, &mut vertices);
        assert!(vertices.is_empty());
    }

    #[test]
    fn rounded_radii_resolve_percentages_and_scale_overlaps() {
        let resolved = resolve_radii(
            &PaintCorners {
                top_left: radius(30.0, 0.0),
                top_right: radius(0.0, 0.5),
                bottom_right: radius(30.0, 0.0),
                bottom_left: radius(30.0, 0.0),
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
            top_left: radius(8.0, 0.0),
            top_right: PaintCornerRadius::default(),
            bottom_right: PaintCornerRadius::default(),
            bottom_left: PaintCornerRadius::default(),
        };
        let mut vertices = Vec::new();
        lower_vertices(
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
            top_left: radius(40.0, 0.0),
            top_right: radius(8.0, 0.0),
            bottom_right: radius(40.0, 0.0),
            bottom_left: radius(8.0, 0.0),
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
        lower_vertices(geometry.outer_rect, &bordered, 1.0, &mut vertices);
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
