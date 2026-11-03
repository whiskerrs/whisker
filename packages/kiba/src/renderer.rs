use std::collections::HashMap;
use std::sync::{LazyLock, Weak};

use glam::Mat4;
use wgpu::util::DeviceExt;

use crate::Mesh;
use crate::mesh::{MeshData, Vertex};

const CLEAR_COLOR: [f64; 3] = [0.02, 0.02, 0.04];
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
// Reversed depth (1 at the near plane) keeps floating-point precision over open-world distances.
const DEPTH_CLEAR: f32 = 0.0;
const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::GreaterEqual;

/// One mesh and the clip-space transforms of its instances.
#[derive(Clone, Debug)]
pub struct Batch {
    pub(crate) mesh: Mesh,
    pub(crate) transforms: Vec<Mat4>,
}

impl Batch {
    /// Draws `mesh` once per transform.
    pub fn new(mesh: Mesh, transforms: Vec<Mat4>) -> Self {
        Self { mesh, transforms }
    }
}

/// The fixed scene of Hosts that do not run a [`Game`](crate::Game) yet.
#[doc(hidden)]
pub fn placeholder_scene() -> [Batch; 1] {
    static TRIANGLE: LazyLock<Mesh> = LazyLock::new(Mesh::triangle);
    [Batch::new(TRIANGLE.clone(), vec![Mat4::IDENTITY])]
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

struct DepthTarget {
    size: (u32, u32),
    view: wgpu::TextureView,
}

impl DepthTarget {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
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
        Self {
            size,
            view: texture.create_view(&Default::default()),
        }
    }
}

/// Encodes the scene into any render target, on a device owned by the caller.
pub struct SceneRenderer {
    pipeline: wgpu::RenderPipeline,
    clear_color: wgpu::Color,
    meshes: HashMap<usize, GpuMesh>,
    instances: Option<wgpu::Buffer>,
    depth: Option<DepthTarget>,
}

impl SceneRenderer {
    /// Prepares the pipeline for targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Encoding in the shader gives the same output on backends without an sRGB surface format.
        let encode_srgb = !format.is_srgb();
        let constants = [("ENCODE_SRGB", f64::from(u8::from(encode_srgb)))];
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
                        ],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: size_of::<Mat4>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            2 => Float32x4,
                            3 => Float32x4,
                            4 => Float32x4,
                            5 => Float32x4,
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
                targets: &[Some(format.into())],
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
        let channel = |linear| {
            if encode_srgb {
                linear_to_srgb(linear)
            } else {
                linear
            }
        };
        Self {
            pipeline,
            clear_color: wgpu::Color {
                r: channel(CLEAR_COLOR[0]),
                g: channel(CLEAR_COLOR[1]),
                b: channel(CLEAR_COLOR[2]),
                a: 1.0,
            },
            meshes: HashMap::new(),
            instances: None,
            depth: None,
        }
    }

    /// Draws one frame into `target`, which measures `size` physical pixels.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        batches: &[Batch],
    ) {
        self.meshes.retain(|_, mesh| mesh.source.strong_count() > 0);
        for batch in batches {
            self.meshes
                .entry(batch.mesh.key())
                .or_insert_with(|| GpuMesh::new(device, &batch.mesh));
        }
        if self.depth.as_ref().is_none_or(|depth| depth.size != size) {
            self.depth = Some(DepthTarget::new(device, size));
        }
        let depth = self.depth.as_ref().expect("allocated above");

        let transforms: Vec<Mat4> = batches
            .iter()
            .flat_map(|batch| batch.transforms.iter().copied())
            .collect();
        let bytes: &[u8] = bytemuck::cast_slice(&transforms);
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

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kiba main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth.view,
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
            let mut first = 0;
            for batch in batches {
                let count = batch.transforms.len() as u64;
                let mesh = &self.meshes[&batch.mesh.key()];
                let stride = size_of::<Mat4>() as u64;
                let range = first * stride..(first + count) * stride;
                first += count;
                // Slicing an empty range panics in wgpu.
                if count == 0 || mesh.index_count == 0 {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, instances.slice(range));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..count as u32);
            }
        }
        queue.submit([encoder.finish()]);
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
        let scene = SceneRenderer::new(&device, format);
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
    pub fn render(&mut self, width: u32, height: u32, batches: &[Batch]) {
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
            Ok(frame) => self.present(frame, batches),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
            }
            Err(error) => log(&error.to_string()),
        }
    }

    fn present(&mut self, frame: wgpu::SurfaceTexture, batches: &[Batch]) {
        let view = frame.texture.create_view(&Default::default());
        let size = (self.config.width, self.config.height);
        self.scene
            .draw(&self.device, &self.queue, &view, size, batches);
        frame.present();
    }
}

fn linear_to_srgb(linear: f64) -> f64 {
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(target_os = "android")]
pub(crate) fn log(message: &str) {
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
pub(crate) fn log(message: &str) {
    eprintln!("kiba: {message}");
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn log(message: &str) {
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
        batches: &[Batch],
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
        SceneRenderer::new(device, format).draw(device, queue, &view, (SIZE, SIZE), batches);

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

    fn solid_quad(color: [f32; 3], depth: f32) -> Batch {
        let vertex = |x, y| Vertex {
            position: [x, y, 0.0],
            color,
        };
        let mesh = Mesh::new(
            vec![
                vertex(-1.0, -1.0),
                vertex(1.0, -1.0),
                vertex(1.0, 1.0),
                vertex(-1.0, 1.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        );
        Batch::new(
            mesh,
            vec![Mat4::from_translation(glam::Vec3::new(0.0, 0.0, depth))],
        )
    }

    #[test]
    fn the_nearest_surface_wins_in_any_draw_order() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let near = solid_quad([1.0, 0.0, 0.0], 0.75);
        let far = solid_quad([0.0, 1.0, 0.0], 0.25);

        for batches in [[near.clone(), far.clone()], [far.clone(), near.clone()]] {
            let pixels =
                render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &batches);
            assert_eq!(center_pixel(&pixels), [255, 0, 0]);
        }
    }

    #[test]
    fn back_faces_are_not_drawn() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let turned =
            Mat4::from_rotation_y(std::f32::consts::PI) * Mat4::from_scale(glam::Vec3::splat(0.5));
        let batch = Batch::new(Mesh::triangle(), vec![turned]);

        let pixels = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &[batch]);

        assert!(center_pixel(&pixels)[0] < 64);
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
        let mut scene = SceneRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let batch = Batch::new(Mesh::triangle(), vec![Mat4::IDENTITY]);

        scene.draw(
            &device,
            &queue,
            &view,
            (SIZE, SIZE),
            std::slice::from_ref(&batch),
        );
        scene.draw(
            &device,
            &queue,
            &view,
            (SIZE, SIZE),
            std::slice::from_ref(&batch),
        );
        assert_eq!(scene.meshes.len(), 1);

        drop(batch);
        scene.draw(&device, &queue, &view, (SIZE, SIZE), &[]);
        assert!(scene.meshes.is_empty());
    }

    #[test]
    fn output_matches_between_srgb_and_unorm_targets() {
        let Some((device, queue)) = gpu() else {
            return;
        };
        let batches = placeholder_scene();

        let encoded = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, &batches);
        let native = render_to_pixels(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &batches,
        );
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
