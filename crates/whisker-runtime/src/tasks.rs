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

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

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

    /// Count of spawned-but-not-yet-completed tasks. Incremented in
    /// [`spawn_local`], decremented when a task's `TrackedTask` wrapper
    /// is dropped (which happens exactly once: on completion OR on the
    /// pool being torn down). Read by [`has_pending_tasks`] so the
    /// driver's tick can report **not-idle** while async work is still
    /// outstanding — see that function's docs for why this matters.
    static OUTSTANDING: Cell<usize> = const { Cell::new(0) };
}

/// Wrapper future that keeps the outstanding-task counter accurate.
///
/// `futures_executor::LocalPool` gives us no completion hook, so we
/// can't decrement `OUTSTANDING` from inside the pool. Instead we wrap
/// every spawned future: the wrapper's `Drop` runs once — when the task
/// finishes and the pool drops it, or when the pool itself is reset —
/// and decrements the counter then. Polling just forwards to the inner
/// future.
struct TrackedTask<F> {
    inner: F,
    counted: bool,
}

impl<F: Future<Output = ()>> Future for TrackedTask<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // SAFETY: standard pin-projection of a `!Unpin`-agnostic field;
        // we never move `inner` out of the pinned wrapper.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll(cx)
    }
}

impl<F> Drop for TrackedTask<F> {
    fn drop(&mut self) {
        if self.counted {
            self.counted = false;
            OUTSTANDING.with(|c| c.set(c.get().saturating_sub(1)));
        }
    }
}

/// Whether the task pool still has work that hasn't run to completion.
///
/// The driver's tick reports "idle" (letting the host pause its render
/// loop) only when there's no reactive work pending. But a `resource()`
/// fetch parked on a [`run_blocking`] worker is *outstanding* — it will
/// be resumed by a cross-thread wake (the worker calling
/// `run_on_main_thread`, which fires `host_wake::wake_runtime`). That
/// wake races against the host pausing its render loop at the end of
/// the very frame that polled the fetch to `Pending`: if the host
/// processes the unpause *before* it processes the end-of-frame pause,
/// the pause clobbers the unpause and the fetch is never re-polled —
/// the resource stays `Loading` forever. (This is the field hang:
/// reactive resource + `run_blocking`, re-fetch after a tracked signal
/// change — see `reactive::tests_resource::cross_thread_wake`.)
///
/// Reporting not-idle while tasks are outstanding closes that race: the
/// host keeps ticking frame-to-frame (as it does during an animation)
/// until the pool drains, so the fetch is always re-polled regardless
/// of how the cross-thread unpause interleaves.
pub fn has_pending_tasks() -> bool {
    OUTSTANDING.with(|c| c.get()) > 0
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
    // Count this task as outstanding and wrap it so the count is
    // decremented exactly once when the task completes (or the pool is
    // reset). `has_pending_tasks` reads this so the driver keeps the
    // host's render loop alive until async work drains — see that fn.
    OUTSTANDING.with(|c| c.set(c.get() + 1));
    let tracked = TrackedTask {
        inner: future,
        counted: true,
    };
    SPAWNER.with(|s| {
        s.borrow()
            .spawn_local(tracked)
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
    // Wrap the receiver so user code awaits `T`. Cancellation (sender
    // dropped because the awaiting owner was disposed) parks the
    // future forever — the task will be GC'd when its owner cascade
    // fires.
    BlockingResult { rx }
}

/// Adapter so `run_blocking`'s return type doesn't leak
/// `futures_channel::oneshot::Receiver`. Pollable as a
/// `Future<Output = T>`; if the sender is dropped (owner disposed
/// mid-fetch) the future parks forever rather than panicking, and the
/// awaiting task is collected when its enclosing owner cascade fires.
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
            // Sender dropped → owner disposed; park forever.
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
    // Replacing the pool drops every queued `TrackedTask`, whose `Drop`
    // decrements `OUTSTANDING`. Force it to 0 afterwards so the counter
    // is exact even if a future was mid-poll / leaked.
    POOL.with(|p| *p.borrow_mut() = LocalPool::new());
    SPAWNER.with(|s| {
        POOL.with(|p| {
            *s.borrow_mut() = p.borrow().spawner();
        });
    });
    OUTSTANDING.with(|c| c.set(0));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_thread::{set_main_thread_dispatcher, DispatchFn};
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::rc::Rc;
    use std::sync::MutexGuard;

    /// Tests in this module reach into thread-local state (executor)
    /// AND process-global state (dispatcher / frame callback). Use the
    /// shared [`crate::main_thread::host_test_lock`] so they don't race
    /// the host-global tests in sibling modules.
    fn lock<'a>() -> MutexGuard<'a, ()> {
        crate::main_thread::host_test_lock()
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
    fn run_on_main_thread_trampoline_wakes_runtime() {
        // Regression for the hn-reader "Loading top stories stuck"
        // bug: a worker thread that calls
        // `run_on_main_thread(|| tx.send(value))` must wake the
        // runtime after the closure runs, so the host re-enters
        // `tick` and `run_until_stalled` polls the awaiting future.
        //
        // Without the wake, the receiver's Waker re-queues the task
        // in `LocalPool`, but `LocalPool` is only polled from `tick`
        // — which doesn't fire if `CADisplayLink` is paused. Result:
        // future sleeps forever, `Resource::state` stays at
        // `Loading`, screen stuck on the loading banner.
        use std::sync::atomic::{AtomicBool, Ordering};
        let _g = lock();
        reset_all();
        install_sync_dispatcher();

        // Install a wake callback that flips a flag.
        static WOKE: AtomicBool = AtomicBool::new(false);
        WOKE.store(false, Ordering::SeqCst);
        extern "C" fn wake_cb(_: *mut c_void) {
            WOKE.store(true, Ordering::SeqCst);
        }
        crate::host_wake::set_request_frame_callback(Some(wake_cb), std::ptr::null_mut());

        // Schedule a no-op closure through `run_on_main_thread`.
        // The sync dispatcher runs it inline; the trampoline must
        // then call `wake_runtime`.
        crate::main_thread::run_on_main_thread(|| {});

        assert!(
            WOKE.load(Ordering::SeqCst),
            "run_on_main_thread's trampoline must wake the runtime \
             after the closure runs — otherwise the awaiting future \
             never gets re-polled (hn-reader Loading-stuck bug)"
        );

        crate::host_wake::__reset_for_tests();
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

    /// `has_pending_tasks` must track the pool's outstanding work so the
    /// driver's tick can report not-idle while a fetch is parked
    /// (closing the cross-thread-wake clobber race). Drives the full
    /// life-cycle: empty → spawned-and-parked → completed → empty.
    #[test]
    fn has_pending_tasks_tracks_outstanding_work() {
        let _g = lock();
        reset_all();
        assert!(!has_pending_tasks(), "empty pool: no pending tasks");

        // A task that parks on first poll (self-woken) then completes on
        // the second — modelling a fetch parked on a cross-thread wake.
        let phase = Rc::new(Cell::new(0));
        let phase_for_task = phase.clone();
        spawn_local(async move {
            Yielder {
                phase: phase_for_task,
                polled_once: false,
            }
            .await;
        });
        assert!(
            has_pending_tasks(),
            "spawned-but-unrun task counts as pending"
        );

        // Yielder self-wakes, so a single run_until_stalled drives both
        // polls to completion → pool drains → no longer pending.
        run_until_stalled();
        assert_eq!(phase.get(), 2, "task ran to completion");
        assert!(
            !has_pending_tasks(),
            "completed task must be removed from the pending count"
        );
    }

    /// A task that stays `Pending` (never woken) keeps the pool
    /// outstanding — so the driver keeps the host alive until it
    /// eventually completes (or its owner is reset).
    #[test]
    fn has_pending_tasks_stays_true_for_parked_task() {
        use std::future;
        let _g = lock();
        reset_all();
        spawn_local(async move {
            future::pending::<()>().await;
        });
        run_until_stalled();
        assert!(
            has_pending_tasks(),
            "a parked (Pending) task remains outstanding after a stalled drain"
        );
        // Reset drops the parked task; the count must zero out.
        __reset_for_tests();
        assert!(
            !has_pending_tasks(),
            "reset must clear the outstanding-task count"
        );
    }
}
