//! FFI entry points the host (Swift `LyraView` on iOS, Kotlin
//! `LyraView` on Android) calls to drive the Rust runtime.

use crate::app_logic::build_demo_tree;
use crate::bridge_ffi::{lyra_bridge_dispatch, LyraEngine};
use crate::bridge_renderer::BridgeRenderer;
use lyra_runtime::render::mount;
use std::ffi::c_void;

/// Per-attach state. Owned by the host while the LyraView is alive.
struct AppContext {
    engine: *mut LyraEngine,
}

/// Bootstrap the runtime against `engine` (which the host obtained via
/// `lyra_bridge_engine_attach`). Returns immediately; rendering happens
/// asynchronously on the Lynx TASM thread.
///
/// `engine_raw` is typed as `*mut c_void` so the public header doesn't
/// have to drag in `lyra_bridge.h`. The pointer is reinterpreted as
/// `*mut LyraEngine` internally — both sides agree on the underlying
/// struct.
///
/// Phase 8 will replace the canned `build_demo_tree` with a runtime that
/// invokes the user's `#[lyra::main]` function on every dirty tick.
#[no_mangle]
pub extern "C" fn lyra_mobile_app_main(engine_raw: *mut c_void) {
    if engine_raw.is_null() {
        return;
    }
    let engine = engine_raw as *mut LyraEngine;
    let ctx = Box::new(AppContext { engine });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { lyra_bridge_dispatch(engine, dispatch_callback, user_data) };
}

extern "C" fn dispatch_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: `user_data` was created by `Box::into_raw` in
    // `lyra_mobile_app_main`. We're consuming it now.
    let ctx: Box<AppContext> = unsafe { Box::from_raw(user_data as *mut AppContext) };

    // SAFETY: We are inside the dispatch callback on the TASM thread,
    // and the engine pointer is valid for the duration of this call (the
    // host keeps the LynxView alive).
    let mut renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    let tree = build_demo_tree("Hello from Rust");
    mount(&mut renderer, &tree);
}
