//! Reusable bootstrap helpers the `#[whisker::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     rsx! { page { text { "Hello" } } }
//! }
//! ```
//!
//! and the macro expands to FFI exports that call [`run`] / [`tick`].

use super::renderer::BridgeRenderer;
use whisker_driver_sys::{whisker_bridge_dispatch, WhiskerEngine};
use whisker_runtime::element::Element;
use whisker_runtime::runtime::Runtime;
use whisker_runtime::signal::set_request_frame_callback;
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
    engine: *mut WhiskerEngine,
    app_fn: BoxedAppFn,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
}

/// Bootstrap the runtime. Called from the FFI export the
/// `#[whisker::main]` macro generates. Users do not call this directly.
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
        engine: engine_raw as *mut WhiskerEngine,
        app_fn: Box::new(app_fn),
        request_frame,
        request_frame_data,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { whisker_bridge_dispatch(engine_raw as *mut WhiskerEngine, init_callback, user_data) };
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

    // The `subsecond::call` wrap, when needed, is now emitted by the
    // `#[whisker::main]` macro inside the user crate (so the wrapper
    // closure ends up in the patch dylib's symbol table, which is
    // the only place the JumpTable can pick it up). We just take the
    // already-wrapped fn here.
    let runtime = Runtime::new(renderer, ctx.app_fn);

    APP_STATE.with(|s| {
        *s.borrow_mut() = Some(AppState { runtime });
    });

    // Once the app is mounted, kick off the dev-server WebSocket
    // receiver. Reads `WHISKER_DEV_ADDR` from the env; a no-op when
    // unset, so even a `hot-reload`-built binary stays inert without
    // an active `whisker run`.
    start_hot_reload_receiver();
}

#[cfg(feature = "hot-reload")]
fn start_hot_reload_receiver() {
    whisker_dev_runtime::start_receiver();
}

#[cfg(not(feature = "hot-reload"))]
fn start_hot_reload_receiver() {}

/// Apply the next pending hot patch, if any. Returns `true` when a
/// patch was successfully applied, so the caller can force a frame
/// even if no signal is dirty — Whisker only redraws on signal changes,
/// and a code swap by itself doesn't mark anything dirty, so without
/// this nudge the screen would keep showing the pre-patch tree until
/// the next user interaction.
#[cfg(feature = "hot-reload")]
fn apply_pending_hot_patch() -> bool {
    let Some(table) = whisker_dev_runtime::take_pending_patch() else {
        return false;
    };
    let entries = table.map.len();
    let lib = table.lib.clone();
    whisker_dev_runtime::devlog(&format!(
        "apply_patch: start (lib={}, entries={entries})",
        lib.display(),
    ));
    let started = std::time::Instant::now();
    // SAFETY: tick_callback runs on the Lynx TASM thread and we
    // call this *before* `runtime.frame()`. The frame is what
    // invokes `subsecond::call`, so no `call` is active here —
    // the only safe window to swap dispatchers.
    match unsafe { subsecond::apply_patch(table) } {
        Ok(()) => {
            whisker_dev_runtime::devlog(&format!(
                "patch applied ({entries} entries in {:?})",
                started.elapsed(),
            ));
            true
        }
        Err(e) => {
            whisker_dev_runtime::devlog(&format!(
                "apply_patch failed: {e:?} (lib was {})",
                lib.display(),
            ));
            false
        }
    }
}

#[cfg(not(feature = "hot-reload"))]
fn apply_pending_hot_patch() -> bool {
    false
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
        whisker_bridge_dispatch(
            engine_raw as *mut WhiskerEngine,
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
    // Drain any pending hot-reload patch *before* the frame runs so
    // the new dispatcher is in place by the time `runtime.frame()`
    // calls `subsecond::call` (a no-op in non-hot-reload builds).
    let patched = apply_pending_hot_patch();
    APP_STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            if patched {
                // Force a re-render so the swapped function bodies
                // run and produce a fresh tree. Without this, Whisker's
                // signal-driven `frame()` short-circuits on `take_dirty()`
                // and the screen stays on the pre-patch UI until the
                // next signal write.
                state.runtime.force_frame();
            } else {
                state.runtime.frame();
            }
        }
    });
    PENDING.with(|p| p.set(false));
}
