//! A forward renderer that draws one [`Scene`] into one surface.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use glam::{Mat4, Vec3, Vec4};
use whisker_surface::{FrameInfo, SurfaceSize};
use whisker_wgpu::{Gpu, Renderer, wgpu};

use crate::core::{
    Geometry, Light, Material, Node, Projection, Scene, Texture, Transform, linear_rgba, srgb_rgba,
};

const MAX_DIRECTIONAL: usize = 4;
const MAX_POINT: usize = 16;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
const MSAA_SAMPLES: u32 = 4;

pub(crate) struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

pub(crate) struct GpuTexture {
    bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    camera_position: [f32; 4],
    ambient: [f32; 4],
    counts: [u32; 4],
    directional: [[[f32; 4]; 2]; MAX_DIRECTIONAL],
    point: [[[f32; 4]; 2]; MAX_POINT],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    base_color: [f32; 4],
    emissive: [f32; 4],
    params: [f32; 4],
}

/// Device-wide objects shared by every scene surface.
struct Shared {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    white: GpuTexture,
    pipelines: RefCell<HashMap<(wgpu::TextureFormat, u32, bool), wgpu::RenderPipeline>>,
}

thread_local! {
    static SHARED: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

fn shared(gpu: &Gpu) -> Rc<Shared> {
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
            entries: &[uniform(false, size_of::<FrameUniform>())],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene draw"),
            entries: &[uniform(true, size_of::<DrawUniform>())],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("whisker-scene texture"),
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
            ..Default::default()
        });
        let white = upload_texture(gpu, &texture_layout, &sampler, 1, 1, &[255; 4]);
        Shared {
            frame_layout,
            draw_layout,
            texture_layout,
            pipeline_layout,
            shader,
            sampler,
            white,
            pipelines: RefCell::new(HashMap::new()),
        }
    }

    fn pipeline(
        &self,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        samples: u32,
        blend: bool,
    ) -> wgpu::RenderPipeline {
        self.pipelines
            .borrow_mut()
            .entry((format, samples, blend))
            .or_insert_with(|| {
                gpu.device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("whisker-scene"),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.shader,
                            entry_point: Some("vs"),
                            compilation_options: Default::default(),
                            buffers: &[Some(wgpu::VertexBufferLayout {
                                array_stride: size_of::<crate::core::Vertex>() as u64,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &wgpu::vertex_attr_array![
                                    0 => Float32x3,
                                    1 => Float32x3,
                                    2 => Float32x2,
                                ],
                            })],
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
                            cull_mode: Some(wgpu::Face::Back),
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

fn upload_texture(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> GpuTexture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("whisker-scene texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("whisker-scene texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuTexture { bind_group }
}

fn mesh<'a>(gpu: &Gpu, geometry: &'a Geometry) -> &'a GpuMesh {
    let data = geometry.data();
    data.gpu.get_or_init(|| {
        let buffer = |label, contents: &[u8], usage| {
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (contents.len() as u64)
                    .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
                    .max(4),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut padded = contents.to_vec();
            padded.resize(buffer.size() as usize, 0);
            gpu.queue.write_buffer(&buffer, 0, &padded);
            buffer
        };
        GpuMesh {
            vertices: buffer(
                "whisker-scene vertices",
                bytemuck::cast_slice(&data.vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "whisker-scene indices",
                bytemuck::cast_slice(&data.indices),
                wgpu::BufferUsages::INDEX,
            ),
            count: data.indices.len() as u32,
        }
    })
}

fn texture<'a>(gpu: &Gpu, shared: &'a Shared, texture: Option<&'a Texture>) -> &'a GpuTexture {
    let Some(texture) = texture else {
        return &shared.white;
    };
    let data = texture.data();
    data.gpu.get_or_init(|| {
        upload_texture(
            gpu,
            &shared.texture_layout,
            &shared.sampler,
            data.width,
            data.height,
            &data.pixels,
        )
    })
}

/// What one frame draws, gathered from the node tree.
#[derive(Default)]
pub(crate) struct Collected {
    pub(crate) draws: Vec<(Mat4, Geometry, Material)>,
    pub(crate) directional: Vec<(Vec3, [f32; 4])>,
    pub(crate) point: Vec<(Vec3, f32, [f32; 4])>,
    pub(crate) cameras: Vec<(Node, Mat4, Projection)>,
}

pub(crate) fn collect(root: &Node) -> Collected {
    let mut collected = Collected::default();
    visit(root, Mat4::IDENTITY, &mut collected);
    collected
}

fn visit(node: &Node, parent: Mat4, out: &mut Collected) {
    if !node.visible() {
        return;
    }
    let world = parent * node.transform().to_matrix();
    if let Some((geometry, material)) = node.mesh() {
        out.draws.push((world, geometry, material));
    }
    match node.light() {
        Some(Light::Directional { color, intensity }) => {
            let toward = -world.transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::Y);
            out.directional.push((toward, radiance(color, intensity)));
        }
        Some(Light::Point {
            color,
            intensity,
            range,
        }) => {
            out.point.push((
                world.transform_point3(Vec3::ZERO),
                range,
                radiance(color, intensity),
            ));
        }
        None => {}
    }
    if let Some(projection) = node.camera() {
        out.cameras.push((node.clone(), world, projection));
    }
    for child in node.children() {
        visit(&child, world, out);
    }
}

fn radiance(color: whisker::css::Color, intensity: f32) -> [f32; 4] {
    let [r, g, b, _] = linear_rgba(color);
    [r * intensity, g * intensity, b * intensity, 0.0]
}

fn default_camera() -> (Mat4, Projection) {
    let transform = Transform::looking_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
    (
        transform.to_matrix(),
        Projection::Perspective {
            fov_y: 60f32.to_radians(),
            near: 0.1,
            far: 1000.0,
        },
    )
}

struct Attachments {
    size: (u32, u32),
    depth: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
}

pub(crate) struct SceneRenderer {
    scene: Scene,
    shared: Rc<Shared>,
    format: wgpu::TextureFormat,
    samples: u32,
    encode_srgb: bool,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    draw_buffer: wgpu::Buffer,
    draw_bind_group: wgpu::BindGroup,
    draw_stride: u64,
    draw_capacity: u64,
    staging: Vec<u8>,
    size: (u32, u32),
    attachments: Option<Attachments>,
}

impl SceneRenderer {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat, scene: Scene) -> SceneRenderer {
        let shared = shared(gpu);
        let multisample = |format: wgpu::TextureFormat| {
            gpu.adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(MSAA_SAMPLES)
        };
        let samples = if multisample(format) && multisample(DEPTH_FORMAT) {
            MSAA_SAMPLES
        } else {
            1
        };
        let frame_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("whisker-scene frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whisker-scene frame"),
            layout: &shared.frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });
        let alignment = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        let draw_stride = (size_of::<DrawUniform>() as u64).next_multiple_of(alignment);
        let (draw_buffer, draw_bind_group) = draw_buffer(gpu, &shared, draw_stride, 16);
        SceneRenderer {
            scene,
            shared,
            format,
            samples,
            encode_srgb: !format.is_srgb(),
            frame_buffer,
            frame_bind_group,
            draw_buffer,
            draw_bind_group,
            draw_stride,
            draw_capacity: 16,
            staging: Vec::new(),
            size: (1, 1),
            attachments: None,
        }
    }

    fn prepare_attachments(&mut self, gpu: &Gpu) {
        if self
            .attachments
            .as_ref()
            .is_some_and(|a| a.size != self.size)
        {
            self.attachments = None;
        }
        let (width, height) = self.size;
        let (format, samples) = (self.format, self.samples);
        self.attachments.get_or_insert_with(|| {
            let texture = |label, format| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            Attachments {
                size: (width, height),
                depth: texture("whisker-scene depth", DEPTH_FORMAT),
                msaa: (samples > 1).then(|| texture("whisker-scene msaa", format)),
            }
        });
    }
}

fn draw_buffer(
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

fn draw_uniform(model: Mat4, material: &Material) -> DrawUniform {
    let normal = model.inverse().transpose();
    let (base_color, emissive, params) = match material {
        Material::Unlit { color } => (linear_rgba(*color), [0.0; 4], [0.0, 1.0, 1.0, 0.0]),
        Material::Standard(standard) => (
            linear_rgba(standard.base_color),
            linear_rgba(standard.emissive),
            [standard.metallic, standard.roughness, 0.0, 0.0],
        ),
    };
    DrawUniform {
        model: model.to_cols_array_2d(),
        normal: normal.to_cols_array_2d(),
        base_color,
        emissive,
        params,
    }
}

fn base_texture(material: &Material) -> Option<&Texture> {
    match material {
        Material::Unlit { .. } => None,
        Material::Standard(standard) => standard.base_color_texture.as_ref(),
    }
}

fn translucent(material: &Material) -> bool {
    let color = match material {
        Material::Unlit { color } => *color,
        Material::Standard(standard) => standard.base_color,
    };
    srgb_rgba(color)[3] < 1.0
}

impl Renderer for SceneRenderer {
    fn resize(&mut self, _gpu: &Gpu, size: SurfaceSize) {
        self.size = (size.width.max(1), size.height.max(1));
    }

    fn render(&mut self, gpu: &Gpu, target: &wgpu::TextureView, frame: FrameInfo) {
        let scene = self.scene.clone();
        let inner = scene.inner();
        inner.run_frame_callbacks(&frame);
        let collected = collect(inner.root());

        let active = inner.active_camera();
        let (camera_world, projection) = collected
            .cameras
            .iter()
            .find(|(node, _, _)| Some(node) == active.as_ref())
            .or(collected.cameras.first())
            .map(|(_, world, projection)| (*world, *projection))
            .unwrap_or_else(default_camera);
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let view = camera_world.inverse();
        let view_proj = projection.to_matrix(aspect) * view;
        let camera_position = camera_world.transform_point3(Vec3::ZERO);

        let (ambient_color, ambient_intensity) = inner.ambient();
        let mut uniform = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            camera_position: camera_position.extend(1.0).to_array(),
            ambient: radiance(ambient_color, ambient_intensity),
            counts: [
                collected.directional.len().min(MAX_DIRECTIONAL) as u32,
                collected.point.len().min(MAX_POINT) as u32,
                u32::from(self.encode_srgb),
                0,
            ],
            directional: [[[0.0; 4]; 2]; MAX_DIRECTIONAL],
            point: [[[0.0; 4]; 2]; MAX_POINT],
        };
        for (slot, (toward, color)) in uniform.directional.iter_mut().zip(&collected.directional) {
            *slot = [toward.extend(0.0).to_array(), *color];
        }
        for (slot, (position, range, color)) in uniform.point.iter_mut().zip(&collected.point) {
            *slot = [position.extend(*range).to_array(), *color];
        }
        gpu.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));

        // Opaque draws first; translucent ones after, farthest first.
        let mut order: Vec<usize> = (0..collected.draws.len()).collect();
        let depth = |index: usize| {
            let center = collected.draws[index].0.transform_point3(Vec3::ZERO);
            (view * center.extend(1.0)).z
        };
        let blended: Vec<bool> = collected
            .draws
            .iter()
            .map(|(_, _, material)| translucent(material))
            .collect();
        order.sort_by(|&a, &b| {
            blended[a].cmp(&blended[b]).then_with(|| match blended[a] {
                true => depth(a).total_cmp(&depth(b)),
                false => std::cmp::Ordering::Equal,
            })
        });

        let count = collected.draws.len() as u64;
        if count > self.draw_capacity {
            let capacity = count.next_power_of_two();
            (self.draw_buffer, self.draw_bind_group) =
                draw_buffer(gpu, &self.shared, self.draw_stride, capacity);
            self.draw_capacity = capacity;
        }
        self.staging.clear();
        self.staging.resize((self.draw_stride * count) as usize, 0);
        for (slot, &index) in order.iter().enumerate() {
            let (model, _, material) = &collected.draws[index];
            let offset = slot * self.draw_stride as usize;
            let uniform = draw_uniform(*model, material);
            let bytes = bytemuck::bytes_of(&uniform);
            self.staging[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        if !self.staging.is_empty() {
            gpu.queue.write_buffer(&self.draw_buffer, 0, &self.staging);
        }

        let shared = self.shared.clone();
        let (format, samples) = (self.format, self.samples);
        let opaque_pipeline = shared.pipeline(gpu, format, samples, false);
        let blend_pipeline = shared.pipeline(gpu, format, samples, true);
        let clear = {
            let rgba = if self.encode_srgb {
                srgb_rgba(inner.background())
            } else {
                linear_rgba(inner.background())
            };
            Vec4::from(rgba).as_dvec4()
        };
        self.prepare_attachments(gpu);
        let Some(attachments) = &self.attachments else {
            return;
        };
        let (view_target, resolve_target) = match &attachments.msaa {
            Some(msaa) => (msaa, Some(target)),
            None => (target, None),
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("whisker-scene"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("whisker-scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: view_target,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear.x,
                            g: clear.y,
                            b: clear.z,
                            a: clear.w,
                        }),
                        store: if resolve_target.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &attachments.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            for (slot, &index) in order.iter().enumerate() {
                let (_, geometry, material) = &collected.draws[index];
                let mesh = mesh(gpu, geometry);
                if mesh.count == 0 {
                    continue;
                }
                pass.set_pipeline(if blended[index] {
                    &blend_pipeline
                } else {
                    &opaque_pipeline
                });
                let offset = (slot as u64 * self.draw_stride) as u32;
                pass.set_bind_group(1, &self.draw_bind_group, &[offset]);
                pass.set_bind_group(
                    2,
                    &texture(gpu, &shared, base_texture(material)).bind_group,
                    &[],
                );
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, 0..1);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests;
