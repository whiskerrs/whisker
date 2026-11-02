const CLEAR_COLOR: [f64; 3] = [0.02, 0.02, 0.04];

/// Encodes the scene into any render target, on a device owned by the caller.
pub struct SceneRenderer {
    pipeline: wgpu::RenderPipeline,
    clear_color: wgpu::Color,
}

impl SceneRenderer {
    /// Prepares the pipeline for targets of `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        // Encoding in the shader gives the same output on backends without an sRGB surface format.
        let encode_srgb = !format.is_srgb();
        let constants = [("ENCODE_SRGB", f64::from(u8::from(encode_srgb)))];
        let shader = device.create_shader_module(wgpu::include_wgsl!("triangle.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kiba triangle"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
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
            primitive: Default::default(),
            depth_stencil: None,
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
        }
    }

    /// Draws one frame into `target`.
    pub fn draw(&self, device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::TextureView) {
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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.draw(0..3, 0..1);
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
    pub fn render(&mut self, width: u32, height: u32) {
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
            Ok(frame) => self.present(frame),
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
            }
            Err(error) => log(&error.to_string()),
        }
    }

    fn present(&self, frame: wgpu::SurfaceTexture) {
        let view = frame.texture.create_view(&Default::default());
        self.scene.draw(&self.device, &self.queue, &view);
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
        SceneRenderer::new(device, format).draw(device, queue, &view);

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

    #[test]
    fn output_matches_between_srgb_and_unorm_targets() {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).expect("device");

        let encoded = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        let native = render_to_pixels(&device, &queue, wgpu::TextureFormat::Rgba8UnormSrgb);

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
