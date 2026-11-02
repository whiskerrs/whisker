use std::ffi::c_void;

use crate::Renderer;
use crate::renderer::log;

/// Creates a renderer drawing into a `CAMetalLayer`, or null on failure.
///
/// # Safety
///
/// `layer` must point to a `CAMetalLayer` that outlives the returned renderer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kiba_ios_create(layer: *mut c_void) -> *mut Renderer {
    let instance = wgpu::Instance::default();
    // SAFETY: the caller guarantees `layer` is a live `CAMetalLayer` that outlives the renderer.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
    };
    let renderer = surface
        .map_err(|error| error.to_string())
        .and_then(|surface| pollster::block_on(Renderer::new(&instance, surface)));
    match renderer {
        Ok(renderer) => Box::into_raw(Box::new(renderer)),
        Err(error) => {
            log(&error);
            std::ptr::null_mut()
        }
    }
}

/// Draws one frame at the given physical size.
///
/// # Safety
///
/// `renderer` must come from [`kiba_ios_create`] and not yet be destroyed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kiba_ios_render(renderer: *mut Renderer, width: u32, height: u32) {
    // SAFETY: the caller guarantees `renderer` is live, and only the main thread calls this.
    unsafe { &mut *renderer }.render(width, height);
}

/// Destroys a renderer.
///
/// # Safety
///
/// `renderer` must come from [`kiba_ios_create`] and is invalid after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kiba_ios_destroy(renderer: *mut Renderer) {
    // SAFETY: the caller transfers back the ownership taken by `Box::into_raw`.
    drop(unsafe { Box::from_raw(renderer) });
}
