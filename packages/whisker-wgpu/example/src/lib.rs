//! `whisker-wgpu` example app.
//!
//! A triangle spins under a Whisker label. The button pauses and resumes frames, which shows whether
//! a Host stops its display link when idle.

use whisker::css::{FlexDirection, FontWeight, Overflow, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_surface::SurfaceView;
use whisker_wgpu::{Gpu, Renderer, wgpu};

const SHADER: &str = r"
struct Uniforms { angle: f32, aspect: f32, encode_srgb: f32 };
@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out { @builtin(position) position: vec4<f32>, @location(0) color: vec3<f32> };

@vertex
fn vs(@builtin(vertex_index) index: u32) -> Out {
    var corners = array<vec2<f32>, 3>(vec2(0.0, 0.7), vec2(-0.6, -0.5), vec2(0.6, -0.5));
    var colors = array<vec3<f32>, 3>(vec3(1.0, 0.3, 0.4), vec3(0.3, 0.9, 0.5), vec3(0.3, 0.5, 1.0));
    let p = corners[index];
    let c = cos(u.angle);
    let s = sin(u.angle);
    var out: Out;
    out.position = vec4(vec2(p.x * c - p.y * s, p.x * s + p.y * c) * vec2(1.0 / u.aspect, 1.0), 0.0, 1.0);
    out.color = colors[index];
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    if u.encode_srgb > 0.5 {
        return vec4(pow(in.color, vec3(1.0 / 2.2)), 1.0);
    }
    return vec4(in.color, 1.0);
}
";

struct Triangle {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    aspect: f32,
    // Targets without an sRGB format, such as GLES on Android, take colors already encoded.
    encode_srgb: bool,
    frames: RwSignal<u64>,
}

impl Triangle {
    fn new(gpu: &Gpu, format: wgpu::TextureFormat, frames: RwSignal<u64>) -> Self {
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("triangle"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangle uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline = gpu
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("triangle"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("triangle"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniforms,
            bind_group,
            aspect: 1.0,
            encode_srgb: !format.is_srgb(),
            frames,
        }
    }
}

impl Renderer for Triangle {
    fn resize(&mut self, _gpu: &Gpu, size: whisker_surface::SurfaceSize) {
        self.aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
    }

    fn render(&mut self, gpu: &Gpu, target: &wgpu::TextureView, frame: whisker_surface::FrameInfo) {
        let angle = frame.timestamp.as_secs_f32();
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&angle.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.aspect.to_le_bytes());
        bytes[8..12].copy_from_slice(&f32::from(u8::from(self.encode_srgb)).to_le_bytes());
        gpu.queue.write_buffer(&self.uniforms, 0, &bytes);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("triangle"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.encode_srgb {
                            wgpu::Color {
                                r: 0.08,
                                g: 0.08,
                                b: 0.11,
                                a: 1.0,
                            }
                        } else {
                            wgpu::Color {
                                r: 0.007,
                                g: 0.007,
                                b: 0.011,
                                a: 1.0,
                            }
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([encoder.finish()]);
        self.frames.update(|frames| *frames += 1);
    }
}

#[whisker::main]
pub fn app() -> Element {
    let frames = signal(0u64);
    let continuous = signal(true);
    let surface = whisker_wgpu::surface(move |gpu, format| Triangle::new(gpu, format, frames));
    surface.set_continuous(true);
    let toggle_surface = surface.clone();
    let adapter = resource(|| async {
        let gpu = whisker_wgpu::gpu()
            .await
            .map_err(|error| error.to_string())?;
        let info = gpu.adapter.get_info();
        Ok(format!("{} ({:?})", info.name, info.backend))
    });

    let page = Css::new()
        .background_color(Color::hex(0x101012))
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(48))
        .padding_left(px(16))
        .padding_right(px(16));
    let title = Css::new()
        .color(Color::hex(0xf0f0f3))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(16));
    let stage = Css::new()
        .height(px(320))
        .border_radius(px(16))
        .overflow(Overflow::Hidden);
    let canvas = Css::new().width(percent(100)).height(percent(100));
    let badge = Css::new()
        .position(PositionKind::Absolute)
        .top(px(12))
        .left(px(12))
        .padding(px(8))
        .border_radius(px(8))
        .background_color(Color::rgba(0, 0, 0, 0.55))
        .color(Color::hex(0xf0f0f3))
        .font_size(px(14));
    let button = Css::new()
        .margin_top(px(16))
        .padding(px(14))
        .border_radius(px(10))
        .background_color(Color::hex(0x5e8df0));
    let button_text = Css::new().color(Color::hex(0xffffff)).font_size(px(16));
    let caption = Css::new()
        .color(Color::hex(0x9a9aa3))
        .font_size(px(13))
        .margin_bottom(px(12));

    render! {
        View(style: page) {
            Text(style: title, value: "whisker-wgpu")
            Text(
                style: caption,
                value: computed(move || match adapter.state() {
                    ResourceState::Loading => "adapter: …".to_string(),
                    ResourceState::Ready(name) => format!("adapter: {name}"),
                    ResourceState::Error(error) => format!("adapter: {error}"),
                }),
            )
            View(style: stage) {
                SurfaceView(surface: surface, style: canvas)
                Text(style: badge, value: computed(move || format!("frames: {}", frames.get())))
            }
            View(
                style: button,
                on_tap: move |_| {
                    let next = !continuous.get_untracked();
                    continuous.set(next);
                    toggle_surface.set_continuous(next);
                    toggle_surface.request_frame();
                },
            ) {
                Text(
                    style: button_text,
                    value: computed(move || if continuous.get() {
                        "Pause".to_string()
                    } else {
                        "Paused: tap to resume".to_string()
                    }),
                )
            }
        }
    }
}
