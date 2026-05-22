//! Reusable bootstrap helpers the `#[whisker::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
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
//!    populates the Lynx element tree and returns an `Element`
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
use whisker_runtime::reactive::{
    flush as reactive_flush, flush_mounts as reactive_flush_mounts, remount_components_for,
};
use whisker_runtime::view::{
    flush as renderer_flush, install_renderer, set_root, DynRenderer, Element,
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
    F: FnOnce() -> Element + 'static,
{
    if engine_raw.is_null() {
        return;
    }
    // Boxed init context, handed across the C ABI via raw pointer.
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut WhiskerEngine,
        app_fn: Some(Box::new(app_fn) as Box<dyn FnOnce() -> Element + 'static>),
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
    app_fn: Option<Box<dyn FnOnce() -> Element + 'static>>,
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
    whisker_runtime::host_wake::set_request_frame_callback(
        ctx.request_frame,
        ctx.request_frame_data,
    );

    // Wire the main-thread dispatcher so background threads can call
    // `run_on_main_thread(|| { ... })` to marshal work onto the TASM
    // thread. The shim erases the `WhiskerEngine*` to `*mut c_void`
    // because `whisker-runtime` doesn't depend on `whisker-driver-sys`.
    whisker_runtime::main_thread::set_main_thread_dispatcher(
        Some(dispatch_shim),
        ctx.engine as *mut c_void,
    );

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

    // Fire on_mount callbacks for everything that just mounted. Has
    // to happen after the renderer flush so user-side code that asks
    // "is my view in the tree?" (e.g. measure-after-layout) sees
    // it. A callback that writes a signal schedules its work via the
    // normal wake-up path; the next tick picks it up.
    reactive_flush_mounts();

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
fn apply_pending_hot_patch() -> Vec<*const ()> {
    let Some(table) = whisker_dev_runtime::take_pending_patch() else {
        return Vec::new();
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
        Ok(patched) => {
            whisker_dev_runtime::devlog(&format!(
                "patch applied ({entries} entries in {:?}, {} fn pointers)",
                started.elapsed(),
                patched.len(),
            ));
            patched
        }
        Err(e) => {
            whisker_dev_runtime::devlog(&format!(
                "apply_patch failed: {e:?} (lib was {})",
                lib.display(),
            ));
            Vec::new()
        }
    }
}

#[cfg(not(feature = "hot-reload"))]
fn apply_pending_hot_patch() -> Vec<*const ()> {
    Vec::new()
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
    // fires. Returns the list of host-side fn pointers that were
    // rewritten; empty if no patch was pending or the patch failed.
    let patched = apply_pending_hot_patch();

    if !patched.is_empty() {
        // Per-component remount: dispose + re-mount every
        // `#[component]` whose fn was patched, so structural
        // changes (new elements, new signals) reflect in the
        // visible tree. State local to the remounted component is
        // lost; state held in context / above the remount point
        // survives.
        remount_components_for(&patched);
    }
    reactive_flush();
    // Drive any async tasks (resource() fetchers, user-spawned
    // futures) until they stall. Tasks that resolve here may write
    // signals; we run another reactive_flush below to surface those
    // writes in the same frame.
    whisker_runtime::tasks::run_until_stalled();
    reactive_flush();
    // Drain on_mount queue *after* the reactive flush — effects that
    // ran this tick may have mounted new components (via `<Show>`
    // flipping true, `<For>` adding an item, etc.), and those
    // newly-mounted components' on_mount callbacks belong to this
    // frame.
    reactive_flush_mounts();
    renderer_flush();
    PENDING.with(|p| p.set(false));
}

/// Type-erased shim handed to `whisker_runtime::main_thread`. The
/// runtime crate stores the engine as `*mut c_void` (it doesn't
/// depend on `whisker-driver-sys`); we cast back here before
/// invoking the C bridge.
extern "C" fn dispatch_shim(
    engine: *mut c_void,
    callback: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
) -> bool {
    if engine.is_null() {
        return false;
    }
    unsafe { whisker_bridge_dispatch(engine as *mut WhiskerEngine, callback, user_data) }
}
