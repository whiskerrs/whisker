//! Post a closure to the Lynx TASM thread (= Whisker's main thread).
//!
//! ## When to use this
//!
//! Background threads can compute values freely, but **`Signal::set` /
//! `effect()` / any other reactive primitive must run on the main
//! thread** — the reactive runtime is thread-local. The typical
//! pattern for "fetch on a worker, render the result" is:
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker::runtime::main_thread::run_on_main_thread;
//!
//! #[component]
//! fn list_view() -> Element {
//!     let data = RwSignal::new(None);
//!
//!     on_mount(move || {
//!         std::thread::spawn(move || {
//!             // worker thread: blocking work, no signal access
//!             let result = fetch_http_blocking("https://...");
//!
//!             // marshal result back to the main thread
//!             run_on_main_thread(move || {
//!                 data.set(Some(result));
//!             });
//!         });
//!     });
//!
//!     render! { /* ... */ }
//! }
//! ```
//!
//! ## Versus `spawn_local`
//!
//! [`spawn_local`](crate::tasks::spawn_local) is the main-thread async
//! executor: it takes a `Future` and polls it on the UI thread.
//! `run_on_main_thread` is the simpler primitive underneath — it takes
//! a plain `FnOnce` and posts it to the main-thread queue, the same
//! idea as Android's `Activity.runOnUiThread(r)`, iOS's
//! `DispatchQueue.main.async {}`, Slint's `invoke_from_event_loop`, or
//! gtk-rs's `MainContext::invoke`.
//!
//! ## How it routes
//!
//! `whisker-driver`'s bootstrap registers a dispatcher
//! ([`set_main_thread_dispatcher`]) that ultimately calls Lynx's
//! `lynx_shell_run_on_tasm_thread` C API. The closure is boxed, the
//! pointer is handed across the C ABI as opaque `user_data`, and a
//! trampoline unboxes + invokes it on the TASM thread.

use std::ffi::c_void;
use std::sync::Mutex;

/// Function-pointer signature of the host-provided dispatcher. Matches
/// the C ABI of `whisker_bridge_dispatch` after erasing the engine
/// pointer type to `*mut c_void` (so this crate doesn't depend on
/// `whisker-driver-sys`).
pub type DispatchFn = extern "C" fn(
    engine: *mut c_void,
    callback: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
) -> bool;

/// Snapshot of the registered dispatcher. Stored globally so any
/// thread can call [`run_on_main_thread`] without thread-local
/// access.
#[derive(Copy, Clone)]
struct Dispatcher {
    func: DispatchFn,
    engine: *mut c_void,
}

/// SAFETY: the engine pointer is an opaque handle owned by the host;
/// the host's contract for `lynx_shell_run_on_tasm_thread` is "safe
/// to call from any thread". The dispatcher itself is a fn pointer.
unsafe impl Send for Dispatcher {}
unsafe impl Sync for Dispatcher {}

static DISPATCHER: Mutex<Option<Dispatcher>> = Mutex::new(None);

/// Optional "drive the runtime now" callback, registered by
/// `whisker-driver::bootstrap`. When set, the [`trampoline`] invokes
/// it (on the main thread, right after the marshaled closure runs)
/// instead of merely requesting a vsync frame. The callback runs the
/// driver's `tick_frame` — flush + drain the task pool + flush +
/// mounts + renderer flush — so an async completion just marshaled
/// onto the main thread is drained and painted on this same
/// main-run-loop post, with the vsync render loop untouched.
///
/// A plain `extern "C" fn()` pointer — no `user_data` needed; the
/// driver's `tick_frame` reads its own thread-locals.
static DRIVE: Mutex<Option<extern "C" fn()>> = Mutex::new(None);

std::thread_local! {
    /// Re-entrancy depth for main-thread render/tick/drive work.
    ///
    /// Some dispatchers (Lynx's `run_on_tasm_thread`, iOS
    /// `Thread.isMainThread` fast paths) invoke the [`trampoline`]
    /// **inline** when called from the TASM thread. Called from inside
    /// the initial render or a `tick_frame`, that would re-enter
    /// `tick_frame` while the renderer / reactive runtime is already
    /// active — a re-entrant borrow that aborts. The depth lets the
    /// trampoline see the nesting and defer instead.
    static MAIN_WORK_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// RAII guard marking that main-thread render/tick/drive work is in
/// progress. The driver wraps `init_callback`'s initial render and every
/// `tick_frame` in one, so a re-entrant `run_on_main_thread` dispatch
/// defers (via the trampoline) instead of nesting.
pub struct MainWorkGuard(());

impl MainWorkGuard {
    pub fn new() -> Self {
        MAIN_WORK_DEPTH.with(|d| d.set(d.get() + 1));
        MainWorkGuard(())
    }
}

impl Default for MainWorkGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MainWorkGuard {
    fn drop(&mut self) {
        MAIN_WORK_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// True while a [`MainWorkGuard`] is alive on this thread — i.e. we are
/// already inside whisker render/tick/drive work and must not re-enter it.
fn main_work_in_progress() -> bool {
    MAIN_WORK_DEPTH.with(|d| d.get()) > 0
}

/// Register the host's main-thread dispatcher. Called once from
/// `whisker-driver::bootstrap` during init. Pass `None` for `func`
/// to clear (used in tests).
#[doc(hidden)]
pub fn set_main_thread_dispatcher(func: Option<DispatchFn>, engine: *mut c_void) {
    let built = func.map(|func| Dispatcher { func, engine });
    if let Ok(mut guard) = DISPATCHER.lock() {
        *guard = built;
    }
}

/// Register the "drive the runtime now" callback (see [`DRIVE`]).
/// Called once from `whisker-driver::bootstrap` during init. Pass
/// `None` to clear (used in tests).
#[doc(hidden)]
pub fn set_drive_callback(cb: Option<extern "C" fn()>) {
    if let Ok(mut guard) = DRIVE.lock() {
        *guard = cb;
    }
}

/// Schedule `f` to run on the Whisker main thread (= Lynx TASM
/// thread) as soon as it services its next message. Safe to call
/// from any thread.
///
/// `f` runs asynchronously — this function returns immediately. The
/// closure is dropped without running if no dispatcher is registered
/// yet (i.e. before bootstrap completes). In that pre-bootstrap
/// window the call is a no-op; debug builds log a warning.
///
/// Inside `f`, the reactive runtime is fully accessible: signal
/// writes, effect registrations, context lookups all work as if you
/// were inside an event handler. Writes that mark new dependencies
/// dirty will wake the host's render loop automatically (via
/// `runtime_wake::wake_runtime` from the scheduler).
///
/// After `f` runs, the [`trampoline`] drives the runtime directly (via
/// the registered [`set_drive_callback`]) on this same main-thread
/// post, so an async result marshaled here (the `run_blocking` /
/// `resource()` path) is drained and painted immediately.
pub fn run_on_main_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    let dispatcher = match DISPATCHER.lock().ok().and_then(|g| *g) {
        Some(d) => d,
        None => {
            #[cfg(debug_assertions)]
            eprintln!(
                "whisker-runtime: run_on_main_thread called before dispatcher \
                 registration; closure dropped"
            );
            return;
        }
    };

    // Double-box: the outer `Box<...>` is what we hand across the C
    // ABI as a raw pointer; the inner `Box<dyn FnOnce>` is what
    // makes the closure type-erased and sized (dyn FnOnce is
    // unsized). The trampoline unboxes both layers and invokes.
    let boxed: Box<Box<dyn FnOnce() + Send + 'static>> = Box::new(Box::new(f));
    let user_data = Box::into_raw(boxed) as *mut c_void;

    let ok = (dispatcher.func)(dispatcher.engine, trampoline, user_data);
    if !ok {
        // Dispatch refused (typically: engine torn down) — reclaim the
        // box so the closure isn't leaked.
        let _: Box<Box<dyn FnOnce() + Send + 'static>> =
            unsafe { Box::from_raw(user_data as *mut Box<dyn FnOnce() + Send + 'static>) };
    }
}

/// Static C-ABI fn the dispatcher invokes on the TASM thread.
extern "C" fn trampoline(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // SAFETY: `run_on_main_thread` is the only producer of
    // `user_data` and it always boxes a `Box<dyn FnOnce>` here.
    let boxed: Box<Box<dyn FnOnce() + Send + 'static>> =
        unsafe { Box::from_raw(user_data as *mut Box<dyn FnOnce() + Send + 'static>) };
    boxed();
    // The closure just run is typically `tx.send(value)` from a
    // `run_blocking` worker: it woke the awaiting future's `Waker` and
    // re-queued it in `LocalPool`, which only re-polls from
    // `run_until_stalled` inside the driver's `tick_frame`. Driving
    // that here — on the host's main-thread post, which the OS services
    // even while the vsync loop is paused — avoids racing an unpause of
    // a paused CADisplayLink / Choreographer.
    //
    // With no drive callback wired (tests that don't model the driver),
    // fall back to requesting a vsync frame.
    //
    // An INLINE trampoline (see `MAIN_WORK_DEPTH`) must defer instead:
    // running `tick_frame` from inside one aborts.
    if main_work_in_progress() {
        crate::runtime_wake::wake_runtime();
        return;
    }
    let drive = DRIVE.lock().ok().and_then(|g| *g);
    match drive {
        Some(cb) => cb(),
        None => crate::runtime_wake::wake_runtime(),
    }
}

/// (Test only) clear the registered dispatcher and drive callback.
#[doc(hidden)]
pub fn __reset_for_tests() {
    if let Ok(mut guard) = DISPATCHER.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = DRIVE.lock() {
        *guard = None;
    }
}

/// Shared serialisation lock for every test that touches the
/// process-global host wiring (the main-thread dispatcher in this
/// module and the frame-request callback in [`crate::runtime_wake`]).
///
/// These globals are reset/installed by tests across SEVERAL modules
/// (`main_thread`, `tasks`, `reactive::tests_resource`). A per-module
/// lock can't keep them from racing — module A could clear the
/// dispatcher mid-fetch in module B, dropping the marshaled result.
/// All such tests take THIS one lock instead.
#[cfg(test)]
pub(crate) fn host_test_lock<'a>() -> std::sync::MutexGuard<'a, ()> {
    static HOST_TEST_LOCK: Mutex<()> = Mutex::new(());
    HOST_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, MutexGuard};

    fn lock<'a>() -> MutexGuard<'a, ()> {
        super::host_test_lock()
    }

    /// Pretend-host dispatcher: invokes the callback synchronously on
    /// the caller's thread.
    extern "C" fn sync_invoke(
        _engine: *mut c_void,
        callback: extern "C" fn(*mut c_void),
        user_data: *mut c_void,
    ) -> bool {
        callback(user_data);
        true
    }

    /// Dispatcher that simulates a failure (engine torn down).
    extern "C" fn refuse(
        _engine: *mut c_void,
        _callback: extern "C" fn(*mut c_void),
        _user_data: *mut c_void,
    ) -> bool {
        false
    }

    fn install(func: DispatchFn) {
        __reset_for_tests();
        set_main_thread_dispatcher(Some(func), std::ptr::null_mut());
    }

    #[test]
    fn closure_runs_when_dispatcher_installed() {
        let _guard = lock();
        install(sync_invoke);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();
        run_on_main_thread(move || {
            ran_clone.store(true, Ordering::SeqCst);
        });
        assert!(ran.load(Ordering::SeqCst), "closure must have run");
        __reset_for_tests();
    }

    #[test]
    fn closure_dropped_when_no_dispatcher() {
        let _guard = lock();
        __reset_for_tests();
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        run_on_main_thread(move || {
            let _ = &flag;
        });
        assert!(
            dropped.load(Ordering::SeqCst),
            "closure (and captured state) must be dropped when no dispatcher is set"
        );
    }

    #[test]
    fn closure_dropped_on_dispatch_failure() {
        let _guard = lock();
        install(refuse);
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        run_on_main_thread(move || {
            let _ = &flag;
        });
        assert!(
            dropped.load(Ordering::SeqCst),
            "closure must be dropped when dispatcher refuses"
        );
        __reset_for_tests();
    }

    #[test]
    fn multiple_dispatches_each_run_once() {
        let _guard = lock();
        install(sync_invoke);
        let counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..5 {
            let c = counter.clone();
            run_on_main_thread(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert_eq!(counter.load(Ordering::SeqCst), 5);
        __reset_for_tests();
    }
}
