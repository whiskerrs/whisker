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
//! ## Why not `spawn_local` / async?
//!
//! `spawn_local` (Leptos, wasm-bindgen-futures, Tauri, …) is a
//! main-thread async executor: it takes a `Future` and polls it on
//! the UI thread. `run_on_main_thread` is the simpler primitive on
//! the other side of the boundary — it takes a plain `FnOnce` and
//! posts it to the main-thread queue. The same idea as Android's
//! `Activity.runOnUiThread(r)`, iOS's `DispatchQueue.main.async {}`,
//! Slint's `invoke_from_event_loop`, or gtk-rs's
//! `MainContext::invoke`.
//!
//! Whisker doesn't run an async executor on the main thread (yet),
//! so we expose only the marshaling primitive. If A4 lands a
//! single-threaded executor later, `spawn_local` will sit on top of
//! this same dispatcher.
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

/// Register the host's main-thread dispatcher. Called once from
/// `whisker-driver::bootstrap` during init. Pass `None` for `func`
/// to clear (used in tests).
#[doc(hidden)]
pub fn set_main_thread_dispatcher(func: Option<DispatchFn>, engine: *mut c_void) {
    eprintln!(
        "[main_thread] set_main_thread_dispatcher(func={}, engine={:p})",
        func.is_some(),
        engine,
    );
    let built = func.map(|func| Dispatcher { func, engine });
    if let Ok(mut guard) = DISPATCHER.lock() {
        *guard = built;
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
/// `host_wake::wake_runtime` from the scheduler).
pub fn run_on_main_thread<F>(f: F)
where
    F: FnOnce() + Send + 'static,
{
    eprintln!("[main_thread] run_on_main_thread called (worker → main hop)");
    let dispatcher = match DISPATCHER.lock().ok().and_then(|g| *g) {
        Some(d) => {
            eprintln!("[main_thread]   dispatcher found, calling .func");
            d
        }
        None => {
            eprintln!(
                "[main_thread] NO dispatcher registered — closure dropped, worker result lost"
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
        // Dispatch refused (typically: engine torn down). Reclaim
        // the box so we don't leak the closure.
        let _: Box<Box<dyn FnOnce() + Send + 'static>> =
            unsafe { Box::from_raw(user_data as *mut Box<dyn FnOnce() + Send + 'static>) };
    }
}

/// Static C-ABI fn the dispatcher invokes on the TASM thread.
extern "C" fn trampoline(user_data: *mut c_void) {
    eprintln!("[main_thread] trampoline entered on TASM thread");
    if user_data.is_null() {
        eprintln!("[main_thread]   user_data null — bailing");
        return;
    }
    // SAFETY: `run_on_main_thread` is the only producer of
    // `user_data` and it always boxes a `Box<dyn FnOnce>` here.
    let boxed: Box<Box<dyn FnOnce() + Send + 'static>> =
        unsafe { Box::from_raw(user_data as *mut Box<dyn FnOnce() + Send + 'static>) };
    boxed();
    eprintln!("[main_thread]   closure ran; calling wake_runtime");
    // Wake the runtime so the host schedules another tick. Without
    // this, a worker thread that calls `run_on_main_thread(|| tx.send(v))`
    // (the `run_blocking` path inside `resource()`) would wake the
    // awaiting future's `Waker` via `tx.send`, which re-queues the
    // future in `LocalPool` — but `LocalPool` only re-polls when
    // `run_until_stalled` is invoked, which the driver only does
    // from the tick callback, which only fires when CADisplayLink is
    // unpaused, which only happens via `request_frame`. So unless we
    // explicitly request a frame here, the awaiting future sleeps
    // forever and `Resource::state` stays at `Loading` even though
    // the worker thread finished. This was the hn-reader
    // "Loading top stories never transitions" bug.
    crate::host_wake::wake_runtime();
}

/// (Test only) clear the registered dispatcher.
#[doc(hidden)]
pub fn __reset_for_tests() {
    if let Ok(mut guard) = DISPATCHER.lock() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};

    /// Tests poke a shared global (the registered dispatcher), so they
    /// must run one at a time. `cargo test` defaults to parallel test
    /// threads — this lock serialises just the tests in this module.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock<'a>() -> MutexGuard<'a, ()> {
        // Unwrap on poison: a poisoned lock means a previous test panicked
        // mid-dispatch — re-running on top of that is fine because we
        // reset state at the start of every test anyway.
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Pretend-host dispatcher: invokes the callback synchronously on
    /// the caller's thread. Good enough to verify the trampoline /
    /// boxing / unbox cycle without spawning a real second thread.
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
        // Closure captures a state we can observe via Drop.
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        run_on_main_thread(move || {
            // Move `flag` in; if the closure is dropped without
            // running, `flag` is also dropped.
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
