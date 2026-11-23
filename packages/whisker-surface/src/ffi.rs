//! C ABI for the Swift and C++ Hosts, and JNI for the Kotlin Host.
//!
//! iOS and macOS export only `whisker_module_*` symbols from the app's Rust framework.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::Duration;

use crate::{NativeSurface, SurfaceSize, host};

const CORE_ANIMATION_LAYER: u32 = 1;
const DCOMP_SURFACE_HANDLE: u32 = 3;

/// # Safety
///
/// `handle` must be a target of `kind` that stays valid until `whisker_module_surface_detach`
/// returns for `id`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whisker_module_surface_attach(
    id: u64,
    kind: u32,
    handle: *mut c_void,
    width: u32,
    height: u32,
    scale: f32,
) {
    let Some(handle) = NonNull::new(handle) else {
        return;
    };
    let target = match kind {
        CORE_ANIMATION_LAYER => NativeSurface::CoreAnimationLayer(handle),
        DCOMP_SURFACE_HANDLE => NativeSurface::DCompSurfaceHandle(handle),
        _ => return,
    };
    host::attach(
        id,
        target,
        SurfaceSize {
            width,
            height,
            scale,
        },
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn whisker_module_surface_resize(id: u64, width: u32, height: u32, scale: f32) {
    host::resize(
        id,
        SurfaceSize {
            width,
            height,
            scale,
        },
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn whisker_module_surface_frame(id: u64, timestamp_seconds: f64) -> bool {
    host::frame(id, Duration::from_secs_f64(timestamp_seconds.max(0.0)))
}

#[unsafe(no_mangle)]
pub extern "C" fn whisker_module_surface_detach(id: u64) {
    host::detach(id);
}

#[cfg(target_os = "android")]
mod android {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::time::Duration;

    use crate::{NativeSurface, SurfaceSize, host};

    #[link(name = "android")]
    unsafe extern "C" {
        fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut c_void;
        fn ANativeWindow_release(window: *mut c_void);
    }

    thread_local! {
        static WINDOWS: RefCell<HashMap<u64, NonNull<c_void>>> = RefCell::new(HashMap::new());
    }

    fn release(id: u64) {
        if let Some(window) = WINDOWS.with(|windows| windows.borrow_mut().remove(&id)) {
            // SAFETY: the map holds the reference `ANativeWindow_fromSurface` took for `id`.
            unsafe { ANativeWindow_release(window.as_ptr()) };
        }
    }

    /// # Safety
    ///
    /// JNI calls this on the main thread with a valid `env` and a live `android.view.Surface`.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn Java_rs_whisker_modules_surface_WhiskerSurfaceView_nativeAttach(
        env: *mut c_void,
        _this: *mut c_void,
        id: i64,
        surface: *mut c_void,
        width: i32,
        height: i32,
        scale: f32,
    ) {
        let id = id as u64;
        host::detach(id);
        release(id);
        // SAFETY: JNI passes a valid `env` and `surface`.
        let Some(window) = NonNull::new(unsafe { ANativeWindow_fromSurface(env, surface) }) else {
            return;
        };
        WINDOWS.with(|windows| windows.borrow_mut().insert(id, window));
        host::attach(
            id,
            NativeSurface::AndroidNativeWindow(window),
            SurfaceSize {
                width: width.max(0) as u32,
                height: height.max(0) as u32,
                scale,
            },
        );
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_rs_whisker_modules_surface_WhiskerSurfaceView_nativeResize(
        _env: *mut c_void,
        _this: *mut c_void,
        id: i64,
        width: i32,
        height: i32,
        scale: f32,
    ) {
        host::resize(
            id as u64,
            SurfaceSize {
                width: width.max(0) as u32,
                height: height.max(0) as u32,
                scale,
            },
        );
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_rs_whisker_modules_surface_WhiskerSurfaceView_nativeFrame(
        _env: *mut c_void,
        _this: *mut c_void,
        id: i64,
        frame_time_nanos: i64,
    ) -> u8 {
        u8::from(host::frame(
            id as u64,
            Duration::from_nanos(frame_time_nanos.max(0) as u64),
        ))
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_rs_whisker_modules_surface_WhiskerSurfaceView_nativeDetach(
        _env: *mut c_void,
        _this: *mut c_void,
        id: i64,
    ) {
        let id = id as u64;
        host::detach(id);
        release(id);
    }
}
