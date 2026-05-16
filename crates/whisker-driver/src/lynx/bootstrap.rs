//! Reusable bootstrap helpers the `#[whisker::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> ElementHandle {
//!     render! { page { text { "Hello" } } }
//! }
//! ```
//!
//! and the macro expands to FFI exports that call [`run`] / [`tick`].
//!
//! ## What happens on mount
//!
//! 1. The C++ bridge dispatches us onto the Lynx TASM thread.
//! 2. We build a `BridgeRenderer` and install it as the thread-local
//!    `DynRenderer` so `view::create_element` / `set_attribute` / …
//!    inside the user's `render!` macro route through the bridge.
//! 3. We invoke `app()`. The user's body runs `render!`, which
//!    populates the Lynx element tree and returns an `ElementHandle`
//!    for the root.
//! 4. We call `view::set_root(root)` and `view::flush()` to commit
//!    the initial frame.
//!
//! ## What happens on tick
//!
//! `tick()` is the host's "you asked us to wake you up" callback. We
//! drain the reactive `flush` queue — running effects whose
//! dependencies have changed since the last tick — then `flush()`
//! the renderer so any element-tree mutations the effects emitted
//! reach the screen. Returns `true` when nothing was pending (the
//! host can park the render loop again).
//!
//! ## Subsecond hot reload
//!
//! On every tick we first try `apply_pending_hot_patch`. If a patch
//! landed, we run the post-patch hook which (in Strategy C, A6) will
//! remount affected components. For now Strategy C is not wired —
//! the patch just swaps function pointers, and unrelated reactive
//! state stays intact.

use super::renderer::BridgeRenderer;
use std::cell::Cell;
use std::ffi::c_void;

use whisker_driver_sys::{whisker_bridge_dispatch, WhiskerEngine};
use whisker_runtime::reactive::flush as reactive_flush;
use whisker_runtime::view::{
    flush as renderer_flush, install_renderer, set_root, ElementHandle, DynRenderer,
};

thread_local! {
    /// `true` between the start of `tick()` and the completion of its
    /// dispatched callback. Used to report idle/busy back to the
    /// host. On our current setup TASM thread == caller thread and
    /// the callback runs synchronously, so this is flipped back to
    /// `false` before `tick()` returns.
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Bootstrap the runtime. Called from the FFI export the
/// `#[whisker::main]` macro generates. Users do not call this
/// directly.
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
    F: FnOnce() -> ElementHandle + 'static,
{
    if engine_raw.is_null() {
        return;
    }
    // Boxed init context, handed across the C ABI via raw pointer.
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut WhiskerEngine,
        app_fn: Some(Box::new(app_fn) as Box<dyn FnOnce() -> ElementHandle + 'static>),
        request_frame,
        request_frame_data,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { whisker_bridge_dispatch(engine_raw as *mut WhiskerEngine, init_callback, user_data) };
}

struct InitCtx {
    engine: *mut WhiskerEngine,
    /// `Option` because we move the closure out inside `init_callback`
    /// to call it. `FnOnce` because the user fn is invoked once at
    /// mount; subsequent re-renders happen incrementally through the
    /// reactive runtime, not by re-calling this fn.
    app_fn: Option<Box<dyn FnOnce() -> ElementHandle + 'static>>,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
}

extern "C" fn init_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let mut ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    // Wire host wake-up before we touch any reactive primitive — any
    // signal writes during the initial `app()` run (lazy state
    // initialisers, eager effects) need to schedule a frame correctly.
    whisker_runtime::signal::set_request_frame_callback(ctx.request_frame, ctx.request_frame_data);

    // Install the bridge renderer into the thread-local before
    // running user code. The `render!` macro's `view::*` calls
    // route through whatever is installed here.
    let _prev = install_renderer(Box::new(renderer) as Box<dyn DynRenderer>);

    // Run the user's app fn (already-`subsecond::call`-wrapped by
    // the macro when the `hot-reload` feature is on).
    let Some(app_fn) = ctx.app_fn.take() else {
        return;
    };
    let root = app_fn();

    // Commit the initial tree: mark the page as root and ask Lynx
    // to run the first layout+paint pass.
    set_root(root);
    renderer_flush();

    start_hot_reload_receiver();
}

#[cfg(feature = "hot-reload")]
fn start_hot_reload_receiver() {
    whisker_dev_runtime::start_receiver();
}

#[cfg(not(feature = "hot-reload"))]
fn start_hot_reload_receiver() {}

/// Apply the next pending hot patch, if any. Returns `true` when a
/// patch was successfully applied so the caller can force a flush
/// even if no signal is dirty — Strategy C (A6) will use this to
/// remount affected components. Until then a hot patch only re-binds
/// effect closure bodies; their re-run is up to whatever future
/// scheduling A6 adds.
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
    // SAFETY: tick_callback runs on the Lynx TASM thread and we call
    // this *before* invoking any user code that might call
    // `subsecond::call`. The only safe window to swap dispatchers.
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

/// Process one frame on demand. Returns `true` when the runtime is
/// fully idle after this tick (no pending effects) so the host can
/// pause its render loop until the next `request_frame` callback
/// fires.
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
    !PENDING.with(|p| p.get())
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    // Drain any pending hot-reload patch before the reactive flush so
    // any patched closures run with their new bodies when the queue
    // fires.
    let _patched = apply_pending_hot_patch();
    // Run any pending effects whose deps changed since the last tick.
    reactive_flush();
    // Commit element-tree mutations the effects produced.
    renderer_flush();
    PENDING.with(|p| p.set(false));
}
