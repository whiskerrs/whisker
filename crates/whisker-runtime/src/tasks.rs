//! Single-threaded async task host.
//!
//! Whisker runs UI on Lynx's TASM thread. To let user code write
//! plain `async fn` (HTTP, animation, debounce, etc.) without pulling
//! in `tokio`, we host a thread-local
//! [`futures_executor::LocalPool`] that's polled cooperatively from
//! the runtime's tick callback.
//!
//! - [`spawn_local`] queues a `Future<Output = ()>` for execution on
//!   the next tick.
//! - [`run_until_stalled`] (crate-internal) drains pending tasks
//!   until they're all blocked on something. The driver's
//!   `whisker_tick` calls this after the reactive flush.
//! - [`run_blocking`] runs a *synchronous* closure on a fresh worker
//!   thread and returns a `Future<Output = T>` that resolves on the
//!   main thread once the closure finishes. Used by [`resource`] and
//!   directly by user code that needs to call sync IO (`ureq`,
//!   filesystem, …) from inside an `async fn`.
//!
//! ## Threading model
//!
//! Everything in this module assumes the local pool lives on the
//! TASM thread. Spawning happens on the TASM thread (so the future
//! itself need not be `Send`). The `Waker` side IS `Send + Sync`
//! because the futures crate requires it, but our concrete wake
//! implementation funnels through [`crate::host_wake::wake_runtime`]
//! which is any-thread safe.
//!
//! [`resource`]: crate::reactive::resource

use std::cell::RefCell;
use std::future::Future;

use futures_executor::{LocalPool, LocalSpawner};
use futures_util::task::LocalSpawnExt;

thread_local! {
    /// The per-thread executor. Holds queued + ready tasks. Polled
    /// to-stall by `run_until_stalled` on every tick.
    ///
    /// We hold `pool` and a cached `spawner` separately so callers of
    /// [`spawn_local`] don't have to mutably borrow the whole pool —
    /// `LocalSpawner` is `Clone` and cheap.
    static POOL: RefCell<LocalPool> = RefCell::new(LocalPool::new());
    static SPAWNER: RefCell<LocalSpawner> = RefCell::new({
        POOL.with(|p| p.borrow().spawner())
    });
}

/// Queue `future` for execution on Whisker's task pool.
///
/// The future is polled on the TASM thread by the next tick after
/// either:
/// - [`crate::host_wake::wake_runtime`] fires (the future's `Waker`
///   was woken from anywhere), or
/// - any other reactive activity drove a tick to begin with.
///
/// The future is not required to be `Send` — Whisker's task pool is
/// strictly single-threaded.
///
/// Returns nothing because once spawned a task is owned by the pool;
/// detach-on-drop is the default. Use [`run_blocking`] when you want
/// a typed result back into an `async fn` body.
///
/// # Panics
///
/// Panics if the local pool has somehow shut down (only possible if
/// a test called `__reset_for_tests` mid-spawn). This is a
/// programming error inside the runtime, never user-reachable.
pub fn spawn_local<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    SPAWNER.with(|s| {
        s.borrow()
            .spawn_local(future)
            .expect("whisker tasks: local pool is shut down");
    });
    // Nudge the host so the next frame's tick callback actually
    // runs and drains the queue. Without this, a freshly spawned
    // task would sit dormant until something else woke the runtime.
    crate::host_wake::wake_runtime();
}

/// Drain pending tasks until they're all pending on external events.
///
/// The driver's tick callback wires this in alongside the reactive
/// flush. Calling it from user code is OK but pointless — the
/// runtime already drives it every tick.
pub fn run_until_stalled() {
    POOL.with(|p| {
        p.borrow_mut().run_until_stalled();
    });
}

/// Offload a synchronous closure to a fresh worker thread and return
/// a future that resolves once the closure completes.
///
/// Use this from inside an `async fn` (or `resource`'s fetcher) when
/// you need to call blocking sync IO (`ureq`, `std::fs`, a sync DB
/// driver) without freezing the TASM thread. The result is delivered
/// back to the main thread via [`crate::main_thread::run_on_main_thread`]
/// so the awaiting future resumes on the TASM thread, not on the
/// worker.
///
/// `T` must be `Send + 'static` so it can cross the thread boundary.
/// `F` must be `Send + 'static` for the same reason.
///
/// ```ignore
/// resource(|| async move {
///     let body = run_blocking(|| {
///         ureq::get("https://example.com").call()
///             .map_err(|e| e.to_string())?
///             .into_string()
///             .map_err(|e| e.to_string())
///     }).await?;
///     parse(body)
/// })
/// ```
pub fn run_blocking<F, T>(closure: F) -> impl Future<Output = T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = futures_channel::oneshot::channel::<T>();
    std::thread::spawn(move || {
        let value = closure();
        // Hop back to the main thread before sending so the receiver
        // wakes up on the TASM thread (same thread the awaiting task
        // polls on). Without this, the receiver would wake from the
        // worker — fine for futures-channel's own internal locking
        // but pessimistic for the runtime: the wake would still go
        // through `host_wake::wake_runtime`, which posts to the main
        // thread, but the order of operations is cleaner this way.
        crate::main_thread::run_on_main_thread(move || {
            // `send` fails only if the awaiting half was dropped
            // (its owner was disposed mid-fetch). In that case the
            // result is simply discarded; no panic, no warning —
            // this is the documented `resource` cancel-on-dispose
            // semantics.
            let _ = tx.send(value);
        });
    });
    // Map the channel error away so the user just awaits `T`.
    // Cancellation produces a never-resolving future (the dropped
    // sender path means the user's awaiting task is dead anyway,
    // so it doesn't matter that we'd panic on `unwrap`). We expose
    // a custom adapter to avoid that panic in tests where the
    // receiver lives but the test ends before the worker writes.
    BlockingResult { rx }
}

/// Adapter so `run_blocking`'s return type doesn't leak
/// `futures_channel::oneshot::Receiver`. Pollable as a
/// `Future<Output = T>`; if the underlying sender is dropped (owner
/// disposed mid-fetch), the future resolves to `T::default()` —
/// **NO**, scratch that — most `T` don't impl `Default`. Instead the
/// future stays `Pending` forever; the awaiting task is then
/// garbage-collected when its owner is disposed.
struct BlockingResult<T> {
    rx: futures_channel::oneshot::Receiver<T>,
}

impl<T> Future for BlockingResult<T> {
    type Output = T;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<T> {
        use std::task::Poll;
        match std::pin::Pin::new(&mut self.rx).poll(cx) {
            Poll::Ready(Ok(v)) => Poll::Ready(v),
            // Sender dropped → owner was disposed. Park forever.
            // The awaiting task will be dropped by the executor
            // once its enclosing future is itself dropped (e.g.
            // when the resource's owner cascade fires).
            Poll::Ready(Err(_)) => Poll::Pending,
            Poll::Pending => Poll::Pending,
        }
    }
}

/// (Test only) reset the executor — drops all queued tasks. Use
/// between unit tests that exercise the pool so leftover work
/// doesn't bleed across.
#[doc(hidden)]
pub fn __reset_for_tests() {
    POOL.with(|p| *p.borrow_mut() = LocalPool::new());
    SPAWNER.with(|s| {
        POOL.with(|p| {
            *s.borrow_mut() = p.borrow().spawner();
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_thread::{set_main_thread_dispatcher, DispatchFn};
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::rc::Rc;
    use std::sync::{Mutex, MutexGuard};

    /// Tests in this module reach into thread-local state (executor)
    /// AND process-global state (dispatcher). Serialise them — the
    /// global dispatcher would otherwise see installs from another
    /// test mid-call.
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn lock<'a>() -> MutexGuard<'a, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset_all() {
        __reset_for_tests();
        crate::main_thread::__reset_for_tests();
    }

    /// Synchronous in-test dispatcher: invokes the callback inline
    /// (on the caller's thread). Good enough to verify run_blocking
    /// without spinning up a real event loop.
    extern "C" fn sync_invoke(
        _engine: *mut c_void,
        callback: extern "C" fn(*mut c_void),
        user_data: *mut c_void,
    ) -> bool {
        callback(user_data);
        true
    }

    fn install_sync_dispatcher() {
        set_main_thread_dispatcher(Some(sync_invoke as DispatchFn), std::ptr::null_mut());
    }

    #[test]
    fn spawn_local_does_not_block_poll_at_call_time() {
        let _g = lock();
        reset_all();
        let flag = Rc::new(Cell::new(false));
        let f = flag.clone();
        spawn_local(async move {
            f.set(true);
        });
        assert!(!flag.get(), "spawn should not poll synchronously");
        run_until_stalled();
        assert!(flag.get(), "tick should drive the task to completion");
    }

    #[test]
    fn run_until_stalled_drains_multiple_independent_tasks() {
        let _g = lock();
        reset_all();
        let counter = Rc::new(Cell::new(0));
        for _ in 0..5 {
            let c = counter.clone();
            spawn_local(async move {
                c.set(c.get() + 1);
            });
        }
        run_until_stalled();
        assert_eq!(counter.get(), 5);
    }

    /// Future that returns Pending on its first poll (re-waking
    /// itself), Ready on the second.
    struct Yielder {
        phase: Rc<Cell<i32>>,
        polled_once: bool,
    }
    impl Future for Yielder {
        type Output = ();
        fn poll(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<()> {
            if !self.polled_once {
                self.polled_once = true;
                self.phase.set(1);
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            } else {
                self.phase.set(2);
                std::task::Poll::Ready(())
            }
        }
    }

    #[test]
    fn run_until_stalled_resumes_self_woken_tasks_within_one_call() {
        let _g = lock();
        reset_all();
        let phase = Rc::new(Cell::new(0));
        let phase_for_task = phase.clone();
        spawn_local(async move {
            Yielder {
                phase: phase_for_task,
                polled_once: false,
            }
            .await;
        });
        run_until_stalled();
        // First poll → Pending + self-wake → second poll → Ready,
        // all inside the same `run_until_stalled` invocation.
        assert_eq!(phase.get(), 2);
    }

    #[test]
    fn run_blocking_returns_value_from_worker_thread() {
        let _g = lock();
        reset_all();
        install_sync_dispatcher();

        let got: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
        let got_for_task = got.clone();
        spawn_local(async move {
            let v = run_blocking(|| 42_i32).await;
            *got_for_task.borrow_mut() = Some(v);
        });

        // The worker thread + sync dispatcher + receiver poll cycle
        // is not synchronous from the spawning thread's POV (the
        // worker may not have scheduled yet). Poll-loop with a cap
        // until the value lands.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while got.borrow().is_none() && std::time::Instant::now() < deadline {
            run_until_stalled();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(*got.borrow(), Some(42));
        crate::main_thread::__reset_for_tests();
    }

    #[test]
    fn run_blocking_future_parks_when_no_dispatcher_registered() {
        let _g = lock();
        reset_all();
        // No dispatcher installed → run_on_main_thread drops the
        // closure, sender never fires, receiver stays Pending
        // forever. The awaiting task body never advances past the
        // .await.

        let polled = Rc::new(Cell::new(false));
        let polled_for_task = polled.clone();
        spawn_local(async move {
            let _v: () = run_blocking(|| {}).await;
            polled_for_task.set(true);
        });
        // Generous wait so worker definitely runs.
        for _ in 0..20 {
            run_until_stalled();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            !polled.get(),
            "task body should NOT have completed without a dispatcher \
             (cancel-on-dispose semantics)"
        );
    }

    #[test]
    fn reset_clears_pending_tasks() {
        let _g = lock();
        reset_all();
        let counter = Rc::new(Cell::new(0));
        let c = counter.clone();
        spawn_local(async move {
            c.set(c.get() + 1);
        });
        // Reset without running — task should be discarded.
        __reset_for_tests();
        run_until_stalled();
        assert_eq!(counter.get(), 0, "reset should drop pending tasks");
    }

    #[test]
    fn spawn_local_after_reset_uses_fresh_spawner() {
        // Regression: __reset_for_tests must rebuild SPAWNER from
        // the fresh POOL. If we accidentally kept the old spawner,
        // subsequent spawn_local calls would fail to enqueue on the
        // new pool.
        let _g = lock();
        reset_all();
        __reset_for_tests();
        let flag = Rc::new(Cell::new(false));
        let f = flag.clone();
        spawn_local(async move {
            f.set(true);
        });
        run_until_stalled();
        assert!(
            flag.get(),
            "spawner must be re-bound to the new pool after reset"
        );
    }
}
