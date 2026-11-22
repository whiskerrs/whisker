//! Draw into Whisker surfaces with wgpu.
//!
//! [`surface`] turns a [`Renderer`] into a [`Surface`] for the `SurfaceView` element. Every
//! surface in the app shares one [`Gpu`], so textures and pipelines can be shared between them.
//!
//! ```ignore
//! let surface = whisker_wgpu::surface(|gpu, format| Triangle::new(gpu, format));
//! render! { SurfaceView(surface: surface, style: css!(width: percent(100), height: px(300))) }
//! ```

mod device;

use std::cell::RefCell;
use std::rc::Rc;

pub use wgpu;
use whisker_surface::{
    FrameInfo, NativeSurface, Surface, SurfaceDelegate, SurfaceSize, WeakSurface,
};

/// The device every surface in the app draws with.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Draws one surface's frames.
pub trait Renderer: 'static {
    fn resize(&mut self, _gpu: &Gpu, _size: SurfaceSize) {}
    fn render(&mut self, gpu: &Gpu, target: &wgpu::TextureView, frame: FrameInfo);
}

/// Why [`gpu`] could not open a device.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuError {
    RequestAdapter(wgpu::RequestAdapterError),
    RequestDevice(wgpu::RequestDeviceError),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuError::RequestAdapter(error) => write!(f, "no suitable GPU adapter: {error}"),
            GpuError::RequestDevice(error) => {
                write!(f, "the GPU device could not be opened: {error}")
            }
        }
    }
}

impl std::error::Error for GpuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GpuError::RequestAdapter(error) => Some(error),
            GpuError::RequestDevice(error) => Some(error),
        }
    }
}

/// The device every surface shares, opened on first use.
///
/// Where the backend needs a window to pick an adapter (GLES on Android, WebGL), this resolves once
/// the first surface connects. A failed attempt is retried by the next call.
pub async fn gpu() -> Result<Gpu, GpuError> {
    device::gpu().await
}

/// A surface drawn by the renderer that `create` returns for the target's texture format.
pub fn surface<R: Renderer>(
    create: impl FnOnce(&Gpu, wgpu::TextureFormat) -> R + 'static,
) -> Surface {
    let state = Rc::new(RefCell::new(State::<R> {
        create: Some(Box::new(create)),
        renderer: None,
        target: None,
        size: None,
        weak: None,
        generation: 0,
    }));
    let surface = Surface::new(Delegate(state.clone()));
    state.borrow_mut().weak = Some(surface.downgrade());
    surface
}

type Create<R> = Box<dyn FnOnce(&Gpu, wgpu::TextureFormat) -> R>;

struct State<R> {
    create: Option<Create<R>>,
    renderer: Option<R>,
    target: Option<Target>,
    size: Option<SurfaceSize>,
    weak: Option<WeakSurface>,
    generation: u64,
}

enum Target {
    Window {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    #[cfg(target_os = "linux")]
    Readback(readback::Readback),
}

impl Target {
    fn format(&self) -> wgpu::TextureFormat {
        match self {
            Target::Window { config, .. } => config
                .view_formats
                .first()
                .copied()
                .unwrap_or(config.format),
            #[cfg(target_os = "linux")]
            Target::Readback(readback) => readback.format(),
        }
    }
}

struct Delegate<R>(Rc<RefCell<State<R>>>);

impl<R: Renderer> SurfaceDelegate for Delegate<R> {
    fn created(&mut self, target: NativeSurface, size: SurfaceSize) {
        let generation = {
            let mut state = self.0.borrow_mut();
            state.generation += 1;
            state.size = Some(size);
            state.generation
        };
        #[cfg(target_arch = "wasm32")]
        {
            let state = self.0.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = connect(target).await;
                finish_connect(&state, generation, result);
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = pollster::block_on(connect(target));
            finish_connect(&self.0, generation, result);
        }
    }

    fn resized(&mut self, size: SurfaceSize) {
        let mut state = self.0.borrow_mut();
        state.size = Some(size);
        let Some(gpu) = device::current() else { return };
        let State {
            target, renderer, ..
        } = &mut *state;
        if let Some(target) = target {
            resize_target(&gpu, target, size);
        }
        if let Some(renderer) = renderer {
            renderer.resize(&gpu, size);
        }
    }

    fn frame(&mut self, frame: FrameInfo) {
        let Some(gpu) = device::current() else { return };
        let mut state = self.0.borrow_mut();
        let State {
            target, renderer, ..
        } = &mut *state;
        let (Some(target), Some(renderer)) = (target, renderer) else {
            return;
        };
        draw(&gpu, target, renderer, frame);
    }

    fn destroyed(&mut self) {
        let mut state = self.0.borrow_mut();
        state.generation += 1;
        state.target = None;
    }
}

fn finish_connect<R: Renderer>(
    state: &Rc<RefCell<State<R>>>,
    generation: u64,
    result: Result<(Gpu, Target), String>,
) {
    let (gpu, mut target) = match result {
        Ok(connected) => connected,
        Err(error) => {
            log(&format!("whisker-wgpu: {error}"));
            return;
        }
    };
    let mut state = state.borrow_mut();
    if state.generation != generation {
        return;
    }
    if let Some(size) = state.size {
        resize_target(&gpu, &mut target, size);
    }
    if let Some(create) = state.create.take() {
        state.renderer = Some(create(&gpu, target.format()));
    }
    let size = state.size;
    if let (Some(renderer), Some(size)) = (&mut state.renderer, size) {
        renderer.resize(&gpu, size);
    }
    state.target = Some(target);
    let weak = state.weak.clone();
    drop(state);
    if let Some(surface) = weak.and_then(|weak| weak.upgrade()) {
        surface.request_frame();
    }
}

async fn connect(target: NativeSurface) -> Result<(Gpu, Target), String> {
    #[cfg(target_os = "linux")]
    if let NativeSurface::Dmabuf(sink) = target {
        let gpu = device::gpu().await.map_err(|error| error.to_string())?;
        return Ok((gpu, Target::Readback(readback::Readback::new(sink))));
    }
    let (gpu, surface) = device::connect(&target).await?;
    let caps = surface.get_capabilities(&gpu.adapter);
    let mut config = surface
        .get_default_config(&gpu.adapter, 1, 1)
        .ok_or("the surface is not supported by the adapter")?;
    if let Some(format) = caps
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
    {
        config.format = format;
    } else if gpu
        .adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        && config.format.add_srgb_suffix() != config.format
    {
        // WebGPU canvases offer only UNORM formats but accept an sRGB view of them.
        config.view_formats.push(config.format.add_srgb_suffix());
    }
    config.present_mode = wgpu::PresentMode::Fifo;
    Ok((gpu, Target::Window { surface, config }))
}

// Linux Hosts only hand out a dmabuf sink, so no arm here matches.
#[cfg_attr(target_os = "linux", allow(unreachable_code, unused_variables))]
pub(crate) fn create_surface(
    instance: &wgpu::Instance,
    target: &NativeSurface,
) -> Result<wgpu::Surface<'static>, String> {
    let surface: Result<_, wgpu::CreateSurfaceError> = match target {
        #[cfg(target_arch = "wasm32")]
        NativeSurface::WebCanvas(canvas) => {
            instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        }
        // SAFETY: whisker-surface keeps every native target valid until `destroyed` returns, and
        // `destroyed` drops this surface.
        #[cfg(target_vendor = "apple")]
        NativeSurface::CoreAnimationLayer(layer) => unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                layer.as_ptr(),
            ))
        },
        // SAFETY: as above.
        #[cfg(windows)]
        NativeSurface::DCompSurfaceHandle(handle) => unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SurfaceHandle(handle.as_ptr()))
        },
        // SAFETY: as above.
        #[cfg(target_os = "android")]
        NativeSurface::AndroidNativeWindow(window) => unsafe {
            use raw_window_handle::{
                AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
            };
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                    *window,
                )),
            })
        },
        _ => return Err("this Host's surface is not supported".into()),
    };
    surface.map_err(|error| error.to_string())
}

fn resize_target(gpu: &Gpu, target: &mut Target, size: SurfaceSize) {
    let (width, height) = (size.width.max(1), size.height.max(1));
    match target {
        Target::Window { surface, config } => {
            config.width = width;
            config.height = height;
            surface.configure(&gpu.device, config);
        }
        #[cfg(target_os = "linux")]
        Target::Readback(readback) => readback.resize(gpu, width, height),
    }
}

fn draw(gpu: &Gpu, target: &mut Target, renderer: &mut impl Renderer, frame: FrameInfo) {
    match target {
        Target::Window { surface, config } => {
            let texture = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    surface.configure(&gpu.device, config);
                    return;
                }
                _ => return,
            };
            let view = texture.texture.create_view(&wgpu::TextureViewDescriptor {
                format: config.view_formats.first().copied(),
                ..Default::default()
            });
            renderer.render(gpu, &view, frame);
            gpu.queue.present(texture);
        }
        #[cfg(target_os = "linux")]
        Target::Readback(readback) => readback.draw(gpu, renderer, frame),
    }
}

#[cfg(target_os = "linux")]
mod readback {
    use whisker_surface::{DmabufSink, FrameInfo};

    use crate::{Gpu, Renderer};

    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    pub(crate) struct Readback {
        sink: DmabufSink,
        texture: Option<(wgpu::Texture, wgpu::Buffer, u32)>,
    }

    impl Readback {
        pub(crate) fn new(sink: DmabufSink) -> Self {
            Self {
                sink,
                texture: None,
            }
        }

        pub(crate) fn format(&self) -> wgpu::TextureFormat {
            FORMAT
        }

        pub(crate) fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("whisker-wgpu readback"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let stride = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("whisker-wgpu readback"),
                size: u64::from(stride) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.texture = Some((texture, buffer, stride));
        }

        pub(crate) fn draw(&mut self, gpu: &Gpu, renderer: &mut impl Renderer, frame: FrameInfo) {
            let Some((texture, buffer, stride)) = &self.texture else {
                return;
            };
            let view = texture.create_view(&Default::default());
            renderer.render(gpu, &view, frame);
            let size = texture.size();
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(*stride),
                        rows_per_image: None,
                    },
                },
                size,
            );
            gpu.queue.submit([encoder.finish()]);
            let slice = buffer.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            if gpu
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .is_err()
            {
                return;
            }
            if let Ok(pixels) = slice.get_mapped_range() {
                self.sink
                    .present_pixels(size.width, size.height, *stride, &pixels);
            }
            buffer.unmap();
        }
    }
}

#[cfg(target_os = "android")]
fn log(message: &str) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(
            priority: std::ffi::c_int,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> std::ffi::c_int;
    }
    const ANDROID_LOG_ERROR: std::ffi::c_int = 6;
    let Ok(text) = std::ffi::CString::new(message) else {
        return;
    };
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { __android_log_write(ANDROID_LOG_ERROR, c"WhiskerWgpu".as_ptr(), text.as_ptr()) };
}

#[cfg(target_arch = "wasm32")]
fn log(message: &str) {
    web_sys::console::error_1(&message.into());
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn log(message: &str) {
    eprintln!("{message}");
}
