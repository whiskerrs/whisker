//! Reusable bootstrap helpers the `#[lyra::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use lyra::prelude::*;
//!
//! #[lyra::main]
//! fn app() -> Element {
//!     rsx! { page { text { "Hello" } } }
//! }
//! ```
//!
//! and the macro expands to FFI exports that call [`run`] / [`tick`].

use crate::bridge_ffi::{lyra_bridge_dispatch, LyraEngine};
use crate::bridge_renderer::BridgeRenderer;
use lyra_runtime::element::Element;
use lyra_runtime::runtime::Runtime;
use std::cell::RefCell;
use std::ffi::c_void;

/// User-supplied app function box. Stored boxed so the persistent
/// `AppState` doesn't have to be generic over the (anonymous) closure
/// type each user provides.
type BoxedAppFn = Box<dyn FnMut() -> Element + 'static>;

struct AppState {
    runtime: Runtime<BridgeRenderer, BoxedAppFn>,
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

/// Trampoline payload — the dispatch callback can't capture closures, so
/// we hand the user's boxed app fn across via `Box::into_raw`.
struct InitCtx {
    engine: *mut LyraEngine,
    app_fn: BoxedAppFn,
}

/// Bootstrap the runtime. Called from the FFI export the
/// `#[lyra::main]` macro generates. Users do not call this directly.
pub fn run<F>(engine_raw: *mut c_void, app_fn: F)
where
    F: FnMut() -> Element + 'static,
{
    if engine_raw.is_null() {
        return;
    }
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut LyraEngine,
        app_fn: Box::new(app_fn),
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { lyra_bridge_dispatch(engine_raw as *mut LyraEngine, init_callback, user_data) };
}

extern "C" fn init_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    let runtime = Runtime::new(renderer, ctx.app_fn);

    APP_STATE.with(|s| {
        *s.borrow_mut() = Some(AppState { runtime });
    });
}

/// Process one frame on demand. Called from the FFI export the
/// `#[lyra::main]` macro generates.
pub fn tick(engine_raw: *mut c_void) {
    if engine_raw.is_null() {
        return;
    }
    unsafe {
        lyra_bridge_dispatch(
            engine_raw as *mut LyraEngine,
            tick_callback,
            std::ptr::null_mut(),
        )
    };
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    APP_STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.runtime.frame();
        }
    });
}
