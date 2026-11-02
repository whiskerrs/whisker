use std::ffi::c_void;
use std::ptr::NonNull;

use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use crate::Renderer;
use crate::renderer::log;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut c_void;
    fn ANativeWindow_release(window: *mut c_void);
}

struct AndroidViewport {
    renderer: Option<Renderer>,
    window: NonNull<c_void>,
}

impl Drop for AndroidViewport {
    fn drop(&mut self) {
        // The wgpu surface holds the window, so it has to go first.
        self.renderer = None;
        // SAFETY: `window` carries the reference taken by `ANativeWindow_fromSurface`.
        unsafe { ANativeWindow_release(self.window.as_ptr()) };
    }
}

fn create(window: NonNull<c_void>) -> Result<Renderer, String> {
    // A Vulkan surface connects the window exclusively, so EGL cannot share an instance with it.
    create_with(window, wgpu::Backends::VULKAN).or_else(|error| {
        log(&format!("falling back to GLES: {error}"));
        create_with(window, wgpu::Backends::GL)
    })
}

fn create_with(window: NonNull<c_void>, backends: wgpu::Backends) -> Result<Renderer, String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });
    // SAFETY: `AndroidViewport` keeps a reference to `window` until after the renderer drops.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: RawDisplayHandle::Android(AndroidDisplayHandle::new()),
            raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(window)),
        })
    }
    .map_err(|error| error.to_string())?;
    pollster::block_on(Renderer::new(&instance, surface))
}

/// # Safety
///
/// JNI calls this with a valid `env` and a live `android.view.Surface`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_rs_whisker_modules_kiba_KibaSurfaceView_nativeCreate(
    env: *mut c_void,
    _this: *mut c_void,
    surface: *mut c_void,
) -> i64 {
    // SAFETY: JNI passes a valid `env` and `surface`.
    let Some(window) = NonNull::new(unsafe { ANativeWindow_fromSurface(env, surface) }) else {
        log("the Surface has no native window");
        return 0;
    };
    let mut viewport = AndroidViewport {
        renderer: None,
        window,
    };
    match create(window) {
        Ok(renderer) => {
            viewport.renderer = Some(renderer);
            Box::into_raw(Box::new(viewport)) as i64
        }
        Err(error) => {
            log(&error);
            0
        }
    }
}

/// # Safety
///
/// `handle` must be a live value returned by `nativeCreate`.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_rs_whisker_modules_kiba_KibaSurfaceView_nativeRender(
    _env: *mut c_void,
    _this: *mut c_void,
    handle: i64,
    width: i32,
    height: i32,
) {
    // SAFETY: the view passes its live handle, and only the main thread calls this.
    let viewport = unsafe { &mut *(handle as *mut AndroidViewport) };
    if let Some(renderer) = &mut viewport.renderer {
        renderer.render(width.max(0) as u32, height.max(0) as u32);
    }
}

/// # Safety
///
/// `handle` must come from `nativeCreate` and is invalid after this call.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_rs_whisker_modules_kiba_KibaSurfaceView_nativeDestroy(
    _env: *mut c_void,
    _this: *mut c_void,
    handle: i64,
) {
    // SAFETY: the view gives back the ownership taken by `Box::into_raw`.
    drop(unsafe { Box::from_raw(handle as *mut AndroidViewport) });
}
