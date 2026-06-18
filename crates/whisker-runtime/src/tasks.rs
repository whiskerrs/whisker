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
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures_executor::{LocalPool, LocalSpawner};
use futures_util::task::{ArcWake, LocalSpawnExt, waker_ref};

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

/// The "request a main-loop drive" hook invoked by a task's [`Waker`]
/// when it is woken from ANY thread.
///
/// # Why this exists
///
/// `LocalPool`'s built-in waker, when woken, only re-queues the task
/// into the pool's internal ready-queue — it does NOT poke the native
/// main loop. The pool is re-polled only from the driver's
/// `tick_frame` → [`run_until_stalled`], which fires on a host vsync
/// `tick()` or on a `run_on_main_thread` DRIVE post. So a task awaiting
/// a future that completes on a FOREIGN thread (e.g. a tokio runtime
/// thread, or a `std::thread` worker) gets re-queued by the wake but
/// never re-polled — it hangs forever while vsync is parked (issue #7).
///
/// We fix this by polling each spawned future through a [`DriveWaker`]
/// that, on `wake`/`wake_by_ref`, forwards to the pool's inner waker
/// (to re-queue the task) AND calls this hook (to poke the main loop).
///
/// In production the hook is [`request_main_loop_drive`], which routes
/// through [`crate::main_thread::run_on_main_thread`] — documented as
/// safe to call from any thread — so the registered DRIVE callback
/// (`tick_frame` → `run_until_stalled`) runs and re-polls the pool.
/// Tests can override it via [`set_drive_hook`] to observe wakes
/// without a full main-loop harness.
type DriveHook = fn();

static DRIVE_HOOK: std::sync::Mutex<DriveHook> = std::sync::Mutex::new(request_main_loop_drive);

/// Production drive hook: ask the host to run another drive of the
/// runtime (which calls [`run_until_stalled`]).
///
/// Routed through `run_on_main_thread(|| {})`: posting an empty
/// closure to the host's main-thread dispatcher makes the trampoline
/// fire the registered DRIVE callback on the main thread, which runs
/// `tick_frame` → `run_until_stalled` and re-polls the ready task.
/// `run_on_main_thread` is any-thread-safe (it snapshots a global
/// dispatcher behind a `Mutex`), so this is sound to call from a
/// foreign thread's wake.
///
/// Re-entrancy: if the wake happens on the MAIN thread while a drive
/// is already in progress (e.g. a task self-wakes during
/// `run_until_stalled`), the trampoline consults
/// `main_work_in_progress()` and defers to a vsync frame instead of
/// re-entering `tick_frame`. The re-queued task is then picked up by
/// the in-flight `run_until_stalled` loop (it drains to a stall) or by
/// the next frame. See `main_thread::trampoline`.
fn request_main_loop_drive() {
    crate::main_thread::run_on_main_thread(|| {});
}

/// Install a custom drive hook. Returns the previous hook so callers
/// (tests) can restore it.
#[doc(hidden)]
pub fn set_drive_hook(hook: DriveHook) -> DriveHook {
    let mut guard = DRIVE_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::replace(&mut *guard, hook)
}

fn invoke_drive_hook() {
    let hook = *DRIVE_HOOK.lock().unwrap_or_else(|e| e.into_inner());
    hook();
}

/// A `Waker` that bridges a foreign-thread wake to Whisker's main
/// loop.
///
/// Holds the pool's own inner `Waker` for the task (so waking still
/// re-queues the task into `LocalPool`'s ready-queue, preserving the
/// stock same-thread / self-wake behavior). On `wake`, it:
///
/// 1. forwards to the inner waker → the task is marked ready; then
/// 2. calls the drive hook → the main loop is poked so
///    `run_until_stalled` actually re-polls.
///
/// Step 2 is the missing link the bare `LocalPool` lacked. It is safe
/// from any thread: the inner waker is `Send + Sync` and the drive
/// hook funnels through the any-thread-safe `run_on_main_thread`.
struct DriveWaker {
    inner: std::task::Waker,
}

impl ArcWake for DriveWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        // (a) mark the task ready in the pool's ready-queue.
        arc_self.inner.wake_by_ref();
        // (b) ensure the main loop will re-poll the pool.
        invoke_drive_hook();
    }
}

/// Wraps a spawned future so that it is always polled with a
/// [`DriveWaker`]-composed `Context`.
///
/// `LocalPool` polls this wrapper with ITS waker (`cx.waker()`). We
/// build a `DriveWaker` around that inner waker and poll the user
/// future with it, so whatever waker the user future stashes (and
/// later wakes, possibly from a foreign thread) is the drive-bridging
/// one — not the bare pool waker. The pool's own scheduling is
/// preserved (the inner waker is still forwarded on every wake); we
/// only ADD the main-loop poke.
struct DriveBridged<F> {
    future: F,
}

impl<F: Future<Output = ()>> Future for DriveBridged<F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // SAFETY: standard pin-projection — we never move `future` out
        // of `self`, and `DriveBridged` is `!Unpin`-agnostic: we only
        // re-pin the field in place.
        let future = unsafe { self.map_unchecked_mut(|s| &mut s.future) };
        let drive_waker = Arc::new(DriveWaker {
            inner: cx.waker().clone(),
        });
        let waker = waker_ref(&drive_waker);
        let mut cx = Context::from_waker(&waker);
        future.poll(&mut cx)
    }
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
            // Wrap so the future is polled with a `DriveWaker`-composed
            // context: a wake from ANY thread re-queues the task in the
            // pool AND pokes the main loop (see `DriveBridged` /
            // `DRIVE_HOOK`). This is what lets a future awaited here
            // resume when woken from a foreign (e.g. tokio) thread.
            .spawn_local(DriveBridged { future })
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
    // Replacing the pool drops every queued task.
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
    use crate::main_thread::{DispatchFn, set_main_thread_dispatcher};
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

    /// A future that parks on its first poll, stashing its `Waker` in a
    /// shared slot, and resolves once an external (foreign-thread)
    /// signal has been set. Models awaiting an I/O completion that is
    /// resolved off the main thread (e.g. a tokio runtime thread).
    struct ForeignSignal {
        done: Arc<std::sync::atomic::AtomicBool>,
        waker_slot: Arc<std::sync::Mutex<Option<std::task::Waker>>>,
    }
    impl Future for ForeignSignal {
        type Output = ();
        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<()> {
            if self.done.load(std::sync::atomic::Ordering::SeqCst) {
                std::task::Poll::Ready(())
            } else {
                // Stash the (drive-bridging) waker so a foreign thread
                // can wake us later.
                *self.waker_slot.lock().unwrap() = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    }

    #[test]
    fn foreign_thread_wake_invokes_drive_hook_and_resumes_task() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let _g = lock();
        reset_all();

        // Inject a test drive hook that records that it was poked. In
        // production this hook routes through `run_on_main_thread`,
        // poking the host main loop; here we just count the pokes and
        // (crucially) re-poll the pool, the way the real DRIVE callback
        // (`tick_frame` → `run_until_stalled`) would.
        static DRIVE_POKES: AtomicUsize = AtomicUsize::new(0);
        DRIVE_POKES.store(0, Ordering::SeqCst);
        fn test_hook() {
            DRIVE_POKES.fetch_add(1, Ordering::SeqCst);
        }
        let prev = set_drive_hook(test_hook);

        let done = Arc::new(AtomicBool::new(false));
        let waker_slot: Arc<std::sync::Mutex<Option<std::task::Waker>>> =
            Arc::new(std::sync::Mutex::new(None));
        let completed = Rc::new(Cell::new(false));

        let done_for_task = done.clone();
        let slot_for_task = waker_slot.clone();
        let completed_for_task = completed.clone();
        spawn_local(async move {
            ForeignSignal {
                done: done_for_task,
                waker_slot: slot_for_task,
            }
            .await;
            completed_for_task.set(true);
        });

        // First drive: polls the task to Pending, stashing its waker.
        run_until_stalled();
        assert!(!completed.get(), "task should be parked awaiting signal");
        assert!(
            waker_slot.lock().unwrap().is_some(),
            "task must have stashed its waker on the first poll"
        );

        // Now complete the future from a SEPARATE std::thread — exactly
        // the foreign-thread wake the bug is about. The thread flips the
        // done flag and wakes the stored (drive-bridging) waker.
        let done_for_thread = done.clone();
        let slot_for_thread = waker_slot.clone();
        let handle = std::thread::spawn(move || {
            done_for_thread.store(true, Ordering::SeqCst);
            let waker = slot_for_thread.lock().unwrap().take().unwrap();
            // Wake from the foreign thread. This must (a) re-queue the
            // task in the pool and (b) invoke the drive hook so the main
            // loop knows to re-poll.
            waker.wake();
        });
        handle.join().unwrap();

        // The drive hook MUST have been invoked by the foreign-thread
        // wake — this is the link the bare LocalPool lacked.
        assert!(
            DRIVE_POKES.load(Ordering::SeqCst) >= 1,
            "foreign-thread wake must invoke the drive hook so the main \
             loop re-polls the pool (issue #7)"
        );

        // Simulate the DRIVE callback the hook would have triggered: a
        // re-poll of the pool. The task is now ready and resolves.
        run_until_stalled();
        assert!(
            completed.get(),
            "task must resume and complete after the foreign-thread wake \
             drove a re-poll"
        );

        set_drive_hook(prev);
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
