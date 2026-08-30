//! Single-threaded async task host.
//!
//! Whisker runs UI on a Host-owned UI thread. To let user code write plain
//! `async fn` without pulling in Tokio, each [`RuntimeContext`](crate::RuntimeContext)
//! owns a [`futures_executor::LocalPool`] that is activated and polled
//! cooperatively from the Host's frame callback.
//!
//! - [`spawn_local`] queues a `Future<Output = ()>` for execution on
//!   the next runtime drive.
//! - [`run_until_stalled`] (crate-internal) drains pending tasks
//!   until they're all blocked on something. `RuntimeInstance` calls this
//!   between reactive flushes.
//! - [`run_blocking`] runs a *synchronous* closure on a fresh worker
//!   thread and returns a `Future<Output = T>` that resolves on the
//!   UI runtime once the closure finishes. Used by [`resource`] and
//!   directly by user code that needs to call sync IO (`ureq`,
//!   filesystem, …) from inside an `async fn`.
//!
//! ## Threading model
//!
//! The local pool is single-threaded and active only while its runtime context
//! is entered, so spawned futures need not be `Send`. The `Waker` side is
//! `Send + Sync`: it captures the instance's any-thread Host wake handle and
//! never polls UI code on the waking thread.
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
    static TASKS: RefCell<TaskState> = RefCell::new(TaskState::new());
}

pub(crate) struct TaskState {
    pool: LocalPool,
    spawner: LocalSpawner,
}

impl TaskState {
    pub(crate) fn new() -> Self {
        let pool = LocalPool::new();
        let spawner = pool.spawner();
        Self { pool, spawner }
    }
}

pub(crate) fn swap_state(state: &mut TaskState) {
    TASKS.with_borrow_mut(|active| std::mem::swap(active, state));
}

/// The "request a main-loop drive" hook invoked by a task's [`Waker`]
/// when it is woken from ANY thread.
///
/// # Why this exists
///
/// `LocalPool`'s built-in waker only re-queues the task into the pool's
/// ready-queue; it does not poke the native main loop. The pool is
/// re-polled only from the driver's `tick_frame` →
/// [`run_until_stalled`], so a task awaiting a future that completes on
/// a FOREIGN thread (a tokio runtime thread, a `std::thread` worker)
/// would be re-queued and never re-polled — hanging for as long as
/// vsync stays parked. Every spawned future is therefore polled through
/// a [`DriveWaker`] that forwards to the pool's inner waker AND calls
/// this hook.
///
/// In production the hook is [`request_main_loop_drive`]. Tests can
/// override it via [`set_drive_hook`] to observe wakes without a full
/// main-loop harness.
type DriveHook = fn();

static DRIVE_HOOK: std::sync::Mutex<DriveHook> = std::sync::Mutex::new(request_main_loop_drive);

/// Production drive hook: ask the host to run another drive of the
/// runtime (which calls [`run_until_stalled`]).
///
/// The Host callback is any-thread-safe and coalesces the request onto its UI
/// event loop. The re-queued task is picked up by the in-flight drain or the
/// next scheduled frame.
fn request_main_loop_drive() {
    crate::runtime_wake::wake_runtime();
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
/// Holds the pool's own inner `Waker` for the task, so waking still
/// re-queues it into `LocalPool`'s ready-queue and the stock
/// same-thread / self-wake behaviour is preserved; then calls the drive
/// hook so the main loop actually re-polls. Safe from any thread — the
/// inner waker is `Send + Sync` and the Host wake contract is any-thread-safe.
struct DriveWaker {
    inner: std::task::Waker,
    wake: Option<crate::runtime_wake::RuntimeWakeHandle>,
}

impl ArcWake for DriveWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.inner.wake_by_ref();
        if let Some(wake) = &arc_self.wake {
            wake.wake();
        } else {
            invoke_drive_hook();
        }
    }
}

/// Wraps a spawned future so that it is always polled with a
/// [`DriveWaker`]-composed `Context`.
///
/// `LocalPool` polls this wrapper with ITS waker (`cx.waker()`); the
/// user future is polled with a `DriveWaker` built around that one, so
/// whatever waker the user future stashes — and later wakes, possibly
/// from a foreign thread — is the drive-bridging one rather than the
/// bare pool waker.
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
            wake: crate::runtime_wake::current_wake_handle(),
        });
        let waker = waker_ref(&drive_waker);
        let mut cx = Context::from_waker(&waker);
        future.poll(&mut cx)
    }
}

/// Queue `future` for execution on Whisker's task pool.
///
/// The future is polled on the Host UI thread by the next drive after
/// either:
/// - [`crate::runtime_wake::wake_runtime`] fires (the future's `Waker`
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
    TASKS.with(|tasks| {
        tasks
            .borrow()
            .spawner
            .spawn_local(DriveBridged { future })
            .expect("whisker tasks: local pool is shut down");
    });
    // Without this nudge a freshly spawned task sits dormant until
    // something else happens to wake the runtime.
    crate::runtime_wake::wake_runtime();
}

/// Drain pending tasks until they're all pending on external events.
///
/// The driver's tick callback wires this in alongside the reactive
/// flush. Calling it from user code is OK but pointless — the
/// runtime already drives it every tick.
pub fn run_until_stalled() {
    TASKS.with(|tasks| {
        tasks.borrow_mut().pool.run_until_stalled();
    });
}

/// Offload a synchronous closure to a fresh worker thread and return
/// a future that resolves once the closure completes.
///
/// Use this from inside an `async fn` (or `resource`'s fetcher) when
/// you need to call blocking sync IO (`ureq`, `std::fs`, a sync DB
/// driver) without freezing the Host UI thread. Sending the result wakes the
/// task's internal drive-aware waker, which
/// requests the correct runtime instance from any thread; the future itself
/// remains polled only on the Host UI thread.
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
        // `send` fails only when the awaiting half was dropped because its
        // owner was disposed mid-fetch. Waking is any-thread safe; polling
        // still happens only when the Host next enters this runtime context.
        let _ = tx.send(value);
    });
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
    TASKS.with_borrow_mut(|tasks| *tasks = TaskState::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::MutexGuard;

    fn lock<'a>() -> MutexGuard<'a, ()> {
        crate::runtime_wake::host_test_lock()
    }

    fn reset_all() {
        __reset_for_tests();
        crate::runtime_wake::__reset_for_tests();
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
        assert_eq!(phase.get(), 2);
    }

    #[test]
    fn run_blocking_returns_value_from_worker_thread() {
        let _g = lock();
        reset_all();

        let got: Rc<RefCell<Option<i32>>> = Rc::new(RefCell::new(None));
        let got_for_task = got.clone();
        spawn_local(async move {
            let v = run_blocking(|| 42_i32).await;
            *got_for_task.borrow_mut() = Some(v);
        });

        // The worker may not have scheduled yet, so poll-loop with a
        // cap until the value lands.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while got.borrow().is_none() && std::time::Instant::now() < deadline {
            run_until_stalled();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(*got.borrow(), Some(42));
    }

    #[test]
    fn run_blocking_completion_does_not_require_legacy_dispatcher() {
        let _g = lock();
        reset_all();
        let polled = Rc::new(Cell::new(false));
        let polled_for_task = polled.clone();
        spawn_local(async move {
            let _v: () = run_blocking(|| {}).await;
            polled_for_task.set(true);
        });
        for _ in 0..20 {
            run_until_stalled();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            polled.get(),
            "the result channel and task waker must work through the Host wake callback"
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

        // The test hook counts pokes; the re-poll the real DRIVE
        // callback would do is done explicitly below.
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

        run_until_stalled();
        assert!(!completed.get(), "task should be parked awaiting signal");
        assert!(
            waker_slot.lock().unwrap().is_some(),
            "task must have stashed its waker on the first poll"
        );

        // Complete the future from a SEPARATE std::thread — the
        // foreign-thread wake this test exists for.
        let done_for_thread = done.clone();
        let slot_for_thread = waker_slot.clone();
        let handle = std::thread::spawn(move || {
            done_for_thread.store(true, Ordering::SeqCst);
            let waker = slot_for_thread.lock().unwrap().take().unwrap();
            waker.wake();
        });
        handle.join().unwrap();

        assert!(
            DRIVE_POKES.load(Ordering::SeqCst) >= 1,
            "foreign-thread wake must invoke the drive hook so the main \
             loop re-polls the pool (issue #7)"
        );

        // Stand in for the DRIVE callback the hook would have triggered.
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
        __reset_for_tests();
        run_until_stalled();
        assert_eq!(counter.get(), 0, "reset should drop pending tasks");
    }

    #[test]
    fn spawn_local_after_reset_uses_fresh_spawner() {
        // `__reset_for_tests` must rebuild SPAWNER from the fresh POOL,
        // or later `spawn_local` calls enqueue onto the dead one.
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
