//! FFI entry points the host (Swift `LyraView` on iOS, Kotlin
//! `LyraView` on Android) calls to drive the Rust runtime.
//!
//! Lifecycle:
//! 1. Host calls `lyra_mobile_app_main(engine)` once after attaching
//!    LynxView. This dispatches to TASM thread and bootstraps a
//!    persistent `Runtime` instance stored in a thread-local on that
//!    thread.
//! 2. Host calls `lyra_mobile_tick(engine)` periodically (e.g. from a
//!    Swift `Timer`). Each call dispatches to the TASM thread, then
//!    runs `Runtime::frame()` so signal updates flush to the screen.

use crate::app_logic::{build_demo_tree, mutate_demo_state};
use crate::bridge_ffi::{lyra_bridge_dispatch, LyraEngine};
use crate::bridge_renderer::BridgeRenderer;
use lyra_runtime::element::Element;
use lyra_runtime::runtime::Runtime;
use lyra_runtime::signal::{use_signal, Signal};
use std::cell::RefCell;
use std::ffi::c_void;

// ----------------------------------------------------------------------------
// Per-app state
// ----------------------------------------------------------------------------

/// State that lives on the TASM thread: the persistent Runtime plus the
/// signals the demo app reads / writes.
struct AppState {
    runtime: Runtime<BridgeRenderer, Box<dyn FnMut() -> Element + 'static>>,
    tick_count: Signal<i32>,
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

// ----------------------------------------------------------------------------
// Initial bootstrap
// ----------------------------------------------------------------------------

/// Per-`lyra_mobile_app_main` payload — passed via `Box::into_raw` so we
/// can carry the engine pointer across the dispatch boundary.
struct InitCtx {
    engine: *mut LyraEngine,
}

/// Bootstrap the Rust runtime against `engine` (which the host obtained
/// via `lyra_bridge_engine_attach`).
///
/// `engine_raw` is typed as `*mut c_void` so the public header doesn't
/// have to drag in `lyra_bridge.h`. The pointer is reinterpreted as
/// `*mut LyraEngine` internally.
#[no_mangle]
pub extern "C" fn lyra_mobile_app_main(engine_raw: *mut c_void) {
    if engine_raw.is_null() {
        return;
    }
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut LyraEngine,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { lyra_bridge_dispatch(engine_raw as *mut LyraEngine, init_callback, user_data) };
}

extern "C" fn init_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    // SAFETY: dispatched onto TASM thread; engine pointer valid for the
    // lifetime of the underlying LynxView (host responsibility).
    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    // Seed the demo signal and stash it for the tick callback.
    let tick_count = use_signal(|| 0_i32);
    let app_fn: Box<dyn FnMut() -> Element + 'static> = Box::new(move || {
        build_demo_tree(&format!("Tick {}", tick_count.get()))
    });
    let runtime = Runtime::new(renderer, app_fn);

    APP_STATE.with(|s| {
        *s.borrow_mut() = Some(AppState {
            runtime,
            tick_count,
        });
    });
}

// ----------------------------------------------------------------------------
// Per-frame tick
// ----------------------------------------------------------------------------

/// Drive one frame from the host. The host calls this on whatever cadence
/// it wants (Swift `Timer`, Choreographer, etc.). Returns immediately;
/// the actual frame work happens on the TASM thread.
#[no_mangle]
pub extern "C" fn lyra_mobile_tick(engine_raw: *mut c_void) {
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
            // Call into the demo's state mutator (separated so the
            // app-level "what counts as a tick" lives next to the tree
            // construction it affects).
            mutate_demo_state(state.tick_count);
            state.runtime.frame();
        }
    });
}
