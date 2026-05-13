//! Reusable bootstrap helpers the `#[tuft::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use tuft::prelude::*;
//!
//! #[tuft::main]
//! fn app() -> Element {
//!     rsx! { page { text { "Hello" } } }
//! }
//! ```
//!
//! and the macro expands to FFI exports that call [`run`] / [`tick`].

use crate::bridge_ffi::{tuft_bridge_dispatch, TuftEngine};
use crate::bridge_renderer::BridgeRenderer;
use tuft_runtime::element::Element;
use tuft_runtime::runtime::Runtime;
use tuft_runtime::signal::set_request_frame_callback;
use std::cell::{Cell, RefCell};
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
    /// Set to `true` by `tick()` and back to `false` once the dispatched
    /// `tick_callback` finishes. Lets `tick()` tell its caller whether the
    /// render actually completed inline (so we can return a meaningful
    /// "idle" answer) or is still in flight on another thread. With our
    /// current iOS shell setup TASM thread == caller thread and the
    /// callback runs synchronously, so this flips false before `tick()`
    /// returns.
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Trampoline payload — the dispatch callback can't capture closures, so
/// we hand the user's boxed app fn (and the host wake-up callback) across
/// via `Box::into_raw`.
struct InitCtx {
    engine: *mut TuftEngine,
    app_fn: BoxedAppFn,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
}

/// Bootstrap the runtime. Called from the FFI export the
/// `#[tuft::main]` macro generates. Users do not call this directly.
///
/// `request_frame` is the host's "wake up the render loop" callback;
/// signal updates fire it so the host can unpause its `CADisplayLink`
/// (or equivalent) to schedule the next tick. May be `None` if the
/// host runs an unconditional render loop.
pub fn run<F>(
    engine_raw: *mut c_void,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
    app_fn: F,
) where
    F: FnMut() -> Element + 'static,
{
    if engine_raw.is_null() {
        return;
    }
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut TuftEngine,
        app_fn: Box::new(app_fn),
        request_frame,
        request_frame_data,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { tuft_bridge_dispatch(engine_raw as *mut TuftEngine, init_callback, user_data) };
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

    // Wire host wake-up first so that any signal writes during the initial
    // app() call (e.g. lazy `use_signal` init) correctly schedule a frame.
    set_request_frame_callback(ctx.request_frame, ctx.request_frame_data);

    let runtime = Runtime::new(renderer, ctx.app_fn);

    APP_STATE.with(|s| {
        *s.borrow_mut() = Some(AppState { runtime });
    });
}

/// Process one frame on demand. Returns `true` when the runtime is fully
/// idle after this tick (nothing dirty) so the host can pause its render
/// loop until the next `request_frame` callback fires.
pub fn tick(engine_raw: *mut c_void) -> bool {
    if engine_raw.is_null() {
        return true;
    }
    PENDING.with(|p| p.set(true));
    unsafe {
        tuft_bridge_dispatch(
            engine_raw as *mut TuftEngine,
            tick_callback,
            std::ptr::null_mut(),
        )
    };
    // If PENDING is now false, the dispatched callback ran inline and we
    // can definitively report idle. Otherwise the callback is still in
    // flight on another thread; conservatively say "not idle" so the host
    // keeps the loop running until the next tick.
    !PENDING.with(|p| p.get())
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    APP_STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.runtime.frame();
        }
    });
    PENDING.with(|p| p.set(false));
}
