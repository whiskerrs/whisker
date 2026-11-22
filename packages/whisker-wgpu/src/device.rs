use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use whisker_surface::NativeSurface;

use crate::{Gpu, GpuError, create_surface};

#[derive(Default)]
struct Shared {
    gpu: Option<Gpu>,
    creating: bool,
    needs_surface: bool,
    settled: u64,
    waiters: Vec<Waker>,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
}

pub(crate) fn current() -> Option<Gpu> {
    SHARED.with(|shared| shared.borrow().gpu.clone())
}

pub(crate) async fn gpu() -> Result<Gpu, GpuError> {
    loop {
        match claim(false) {
            Claim::Ready(gpu) => return Ok(gpu),
            Claim::Wait(seen) => Settled(seen).await,
            Claim::Create(creating) => match create().await {
                Ok(gpu) => {
                    creating.finish(Some(gpu.clone()), false);
                    return Ok(gpu);
                }
                Err(Failure::NeedsSurface) => creating.finish(None, true),
                Err(Failure::Gpu(error)) => {
                    creating.finish(None, false);
                    return Err(error);
                }
            },
        }
    }
}

/// The shared device and a surface for `target` made from its instance.
pub(crate) async fn connect(
    target: &NativeSurface,
) -> Result<(Gpu, wgpu::Surface<'static>), String> {
    loop {
        match claim(true) {
            Claim::Ready(gpu) => {
                let surface = create_surface(&gpu.instance, target)?;
                return Ok((gpu, surface));
            }
            Claim::Wait(seen) => Settled(seen).await,
            Claim::Create(creating) => {
                return match create_with_surface(target).await {
                    Ok((gpu, surface)) => {
                        creating.finish(Some(gpu.clone()), false);
                        Ok((gpu, surface))
                    }
                    Err(error) => {
                        creating.finish(None, false);
                        Err(error)
                    }
                };
            }
        }
    }
}

enum Claim {
    Ready(Gpu),
    Wait(u64),
    Create(Creating),
}

fn claim(with_surface: bool) -> Claim {
    SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        if let Some(gpu) = &shared.gpu {
            return Claim::Ready(gpu.clone());
        }
        if shared.creating || (shared.needs_surface && !with_surface) {
            return Claim::Wait(shared.settled);
        }
        shared.creating = true;
        Claim::Create(Creating { done: false })
    })
}

/// Releases the claim when its future is dropped mid-creation, or every later caller would wait forever.
struct Creating {
    done: bool,
}

impl Creating {
    fn finish(mut self, gpu: Option<Gpu>, needs_surface: bool) {
        self.done = true;
        settle(gpu, needs_surface);
    }
}

impl Drop for Creating {
    fn drop(&mut self) {
        if !self.done {
            settle(None, false);
        }
    }
}

fn settle(gpu: Option<Gpu>, needs_surface: bool) {
    let waiters = SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        shared.gpu = gpu;
        shared.creating = false;
        shared.needs_surface = needs_surface;
        shared.settled += 1;
        std::mem::take(&mut shared.waiters)
    });
    for waiter in waiters {
        waiter.wake();
    }
}

struct Settled(u64);

impl Future for Settled {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        SHARED.with(|shared| {
            let mut shared = shared.borrow_mut();
            if shared.settled != self.0 {
                Poll::Ready(())
            } else {
                shared.waiters.push(cx.waker().clone());
                Poll::Pending
            }
        })
    }
}

enum Failure {
    #[cfg_attr(
        not(any(target_os = "android", target_arch = "wasm32")),
        allow(dead_code)
    )]
    NeedsSurface,
    Gpu(GpuError),
}

async fn create() -> Result<Gpu, Failure> {
    let instance = new_instance().await;
    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
    {
        Ok(adapter) => adapter,
        // GLES and WebGL pick an adapter through a window, so this resolves once a surface connects.
        #[cfg(any(target_os = "android", target_arch = "wasm32"))]
        Err(_) => return Err(Failure::NeedsSurface),
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        Err(error) => return Err(Failure::Gpu(GpuError::RequestAdapter(error))),
    };
    open(instance, adapter).await.map_err(Failure::Gpu)
}

async fn create_with_surface(
    target: &NativeSurface,
) -> Result<(Gpu, wgpu::Surface<'static>), String> {
    let instance = new_instance().await;
    let surface = create_surface(&instance, target)?;
    let adapter = request_adapter(&instance, &surface).await;
    #[cfg(target_os = "android")]
    let (instance, surface, adapter) = match adapter {
        Ok(adapter) => (instance, surface, Ok(adapter)),
        // A Vulkan surface takes the window exclusively, so GLES needs its own instance and surface.
        Err(_) => {
            drop(surface);
            let gles = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });
            let surface = create_surface(&gles, target)?;
            let adapter = request_adapter(&gles, &surface).await;
            (gles, surface, adapter)
        }
    };
    let adapter = adapter.map_err(|error| error.to_string())?;
    let gpu = open(instance, adapter)
        .await
        .map_err(|error| error.to_string())?;
    Ok((gpu, surface))
}

async fn request_adapter(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
) -> Result<wgpu::Adapter, wgpu::RequestAdapterError> {
    instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(surface),
            ..Default::default()
        })
        .await
}

async fn open(instance: wgpu::Instance, adapter: wgpu::Adapter) -> Result<Gpu, GpuError> {
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            ..Default::default()
        })
        .await
        .map_err(GpuError::RequestDevice)?;
    Ok(Gpu {
        instance,
        adapter,
        device,
        queue,
    })
}

#[allow(clippy::unused_async)]
async fn new_instance() -> wgpu::Instance {
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::util::new_instance_with_webgpu_detection(
            wgpu::InstanceDescriptor::new_without_display_handle(),
        )
        .await
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[allow(unused_mut)]
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        #[cfg(target_os = "android")]
        {
            descriptor.backends = wgpu::Backends::VULKAN;
        }
        // DirectComposition surface handles only reach DX12.
        #[cfg(windows)]
        {
            descriptor.backends = wgpu::Backends::DX12;
        }
        wgpu::Instance::new(descriptor)
    }
}
