use std::time::Duration;

use super::*;

const SIZE: u32 = 64;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

const SHADER: &str = r#"
struct Uniform { matrix: mat4x4<f32>, color: vec4<f32> }
@group(0) @binding(0) var<uniform> layer: Uniform;

fn corner(index: u32) -> vec2<f32> {
    var corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    return corners[index];
}

@vertex fn vs_wall(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let c = corner(index);
    return vec4(c.x - 1.0, c.y * 2.0 - 1.0, 0.25, 1.0);
}

@fragment fn fs_wall() -> @location(0) vec4<f32> {
    return vec4(1.0, 0.0, 0.0, 1.0);
}

@vertex fn vs_layer(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let c = corner(index) * 2.0 - 1.0;
    return layer.matrix * vec4(c, 0.5, 1.0);
}

@fragment fn fs_layer() -> @location(0) vec4<f32> {
    return layer.color;
}
"#;

fn with_gpu(test: impl FnOnce(&Gpu)) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
        ..Default::default()
    }))
    .unwrap();
    test(&Gpu {
        instance,
        adapter,
        device,
        queue,
    });
}

fn pipeline(
    gpu: &Gpu,
    layout: Option<&wgpu::PipelineLayout>,
    entry: &str,
    depth: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(&format!("vs_{entry}")),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(&format!("fs_{entry}")),
                compilation_options: Default::default(),
                targets: &[Some(COLOR.into())],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

/// Draws a quad behind the host's wall, in a color its `prepare` copies into place.
struct Quad {
    color: [f32; 4],
    state: Option<QuadState>,
}

struct QuadState {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    staging: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Layer for Quad {
    fn prepare(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, target: &LayerTarget) {
        let state = self.state.get_or_insert_with(|| {
            let layout = gpu
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
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
            let pipeline_layout =
                gpu.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
            let buffer = |usage| {
                gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 80,
                    usage,
                    mapped_at_creation: false,
                })
            };
            let uniform = buffer(wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
            let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            QuadState {
                pipeline: pipeline(gpu, Some(&pipeline_layout), "layer", target.depth_format),
                staging: buffer(wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST),
                uniform,
                bind_group,
            }
        });
        let mut bytes = Vec::with_capacity(80);
        for column in target.view {
            for value in column {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in self.color {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        gpu.queue.write_buffer(&state.staging, 0, &bytes);
        encoder.copy_buffer_to_buffer(&state.staging, 0, &state.uniform, 0, 80);
    }

    fn draw(&mut self, _gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>, _target: &LayerTarget) {
        let state = self.state.as_ref().expect("drawn before prepared");
        pass.set_pipeline(&state.pipeline);
        pass.set_bind_group(0, &state.bind_group, &[]);
        pass.draw(0..6, 0..1);
    }
}

/// A host that draws a wall over the left half at depth 0.25, then `layer`, into a fresh target.
fn host_frame(gpu: &Gpu, layer: &LayerHandle) -> Vec<[u8; 4]> {
    let target = LayerTarget {
        view: IDENTITY,
        projection: IDENTITY,
        frame: FrameInfo {
            timestamp: Duration::ZERO,
        },
        color_format: COLOR,
        depth_format: DEPTH,
        samples: 1,
        size: (SIZE, SIZE),
        scale: 1.0,
    };
    let texture = |format, usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
            view_formats: &[],
        })
    };
    let color = texture(COLOR, wgpu::TextureUsages::COPY_SRC);
    let depth = texture(DEPTH, wgpu::TextureUsages::empty());
    let wall = pipeline(gpu, None, "wall", DEPTH);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    layer.borrow_mut().prepare(gpu, &mut encoder, &target);
    {
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            }),
            ..Default::default()
        });
        pass.set_pipeline(&wall);
        pass.draw(0..6, 0..1);
        layer.borrow_mut().draw(gpu, &mut pass, &target);
    }
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(SIZE * SIZE * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        color.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    bytes
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .collect()
}

#[test]
fn a_layer_draws_into_the_host_pass_behind_what_the_host_drew_in_front() {
    with_gpu(|gpu| {
        let layer = LayerHandle::new(Quad {
            color: [0.0, 1.0, 0.0, 1.0],
            state: None,
        });
        let pixels = host_frame(gpu, &layer);
        let at = |x: u32| pixels[(SIZE / 2 * SIZE + x) as usize];
        assert_eq!(
            at(8),
            [255, 0, 0, 255],
            "the host's nearer wall hides the layer"
        );
        assert_eq!(
            at(56),
            [0, 255, 0, 255],
            "the layer draws where nothing is nearer"
        );
    });
}

#[test]
fn handles_compare_by_identity() {
    let quad = || Quad {
        color: [0.0; 4],
        state: None,
    };
    let a = LayerHandle::new(quad());
    assert_eq!(a, a.clone());
    assert_ne!(a, LayerHandle::new(quad()));
}

fn votes(host: &LayerHost) -> usize {
    host.surface.continuous.borrow().len()
}

#[test]
fn continuity_holds_while_any_host_handle_asks_for_it() {
    struct Idle;
    impl whisker_surface::SurfaceDelegate for Idle {
        fn created(&mut self, _: whisker_surface::NativeSurface, _: whisker_surface::SurfaceSize) {}
        fn resized(&mut self, _: whisker_surface::SurfaceSize) {}
        fn frame(&mut self, _: FrameInfo) {}
        fn destroyed(&mut self) {}
    }
    let surface = whisker_surface::Surface::new(Idle);
    let host = LayerHost::new(surface.downgrade());
    let layer = host.separate();
    host.set_continuous(true);
    layer.set_continuous(true);
    host.set_continuous(false);
    assert_eq!(votes(&host), 1, "the layer still asks");
    let clone = layer.clone();
    drop(layer);
    assert_eq!(votes(&host), 1, "a clone keeps its handle's request");
    drop(clone);
    assert_eq!(votes(&host), 0, "dropping the last clone withdraws it");
}
