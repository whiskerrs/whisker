//! Unit tests for `resource_sync` + the async `resource()`.
//!
//! `resource()` polls its `async` fetcher on the runtime's task pool
//! ([`crate::tasks`]); tests drive the pool with
//! [`crate::tasks::run_until_stalled`] to make the async path
//! observable without needing an active main-thread dispatcher (for
//! purely-async fetchers; `run_blocking`-using fetchers do require
//! the dispatcher and are exercised in `tasks::tests`).

use crate::reactive::RwSignal;
use crate::reactive::{__reset_for_tests, flush, resource, resource_sync, Owner, ResourceState};
use crate::tasks;

fn with_test_owner<R>(f: impl FnOnce() -> R) -> R {
    __reset_for_tests();
    tasks::__reset_for_tests();
    let owner = Owner::new(None);
    owner.with(f)
}

#[test]
fn resource_sync_ready_state_for_ok_fetch() {
    with_test_owner(|| {
        let r = resource_sync(|| Ok::<_, String>(42_i32));
        assert!(matches!(r.state(), ResourceState::Ready(42)));
        assert_eq!(r.get(), Some(42));
        assert!(!r.loading());
        assert!(r.error().is_none());
    });
}

#[test]
fn resource_sync_error_state_for_err_fetch() {
    with_test_owner(|| {
        let r = resource_sync(|| Err::<i32, _>("oops".to_string()));
        assert!(matches!(r.state(), ResourceState::Error(_)));
        assert_eq!(r.get(), None);
        assert!(!r.loading());
        assert_eq!(r.error().as_deref(), Some("oops"));
    });
}

#[test]
fn async_resource_starts_in_loading_state() {
    // resource() returns before the fetcher's future has had a
    // chance to be polled — the state must be Loading at call
    // time regardless of how short the fetcher is.
    with_test_owner(|| {
        let r = resource::<i32, _, _>(|| async { Ok(7) });
        assert!(r.loading());
        assert!(matches!(r.state(), ResourceState::Loading));
        assert_eq!(r.get(), None);
        assert!(r.error().is_none());
    });
}

#[test]
fn async_resource_transitions_to_ready_after_tick() {
    // After one `run_until_stalled`, a fetcher whose future
    // resolves on first poll should have written its value.
    with_test_owner(|| {
        let r = resource::<i32, _, _>(|| async { Ok(99) });
        tasks::run_until_stalled();
        assert!(matches!(r.state(), ResourceState::Ready(99)));
        assert_eq!(r.get(), Some(99));
        assert!(!r.loading());
    });
}

#[test]
fn async_resource_transitions_to_error_on_err_result() {
    with_test_owner(|| {
        let r = resource::<i32, _, _>(|| async { Err("boom".to_string()) });
        tasks::run_until_stalled();
        assert!(matches!(r.state(), ResourceState::Error(_)));
        assert_eq!(r.error().as_deref(), Some("boom"));
        assert!(!r.loading());
    });
}

#[test]
fn async_resource_with_pending_future_stays_loading() {
    // Fetcher whose future returns Pending on first poll without
    // ever waking — resource should still be in Loading state
    // after run_until_stalled.
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct NeverReady;
    impl std::future::Future for NeverReady {
        type Output = Result<i32, String>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    with_test_owner(|| {
        let r = resource::<i32, _, _>(|| NeverReady);
        tasks::run_until_stalled();
        assert!(r.loading(), "never-ready future must keep resource Loading");
    });
}

#[test]
fn async_resource_multi_step_future_completes_within_one_tick() {
    // Future that yields once before resolving — single
    // `run_until_stalled` should drive both polls because the
    // first wake re-schedules immediately.
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct OneYield(bool);
    impl std::future::Future for OneYield {
        type Output = Result<i32, String>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if !self.0 {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(Ok(123))
            }
        }
    }

    with_test_owner(|| {
        let r = resource::<i32, _, _>(|| OneYield(false));
        tasks::run_until_stalled();
        assert_eq!(r.get(), Some(123));
    });
}

#[test]
fn async_resource_tracks_signal_read_in_sync_prefix_and_refetches() {
    // (a) Sync-prefix tracking: the fetcher reads `query` BEFORE its
    // `.await`, deriving the fetched value from it. Changing `query`
    // must re-run the fetcher and update the resource with the new
    // value. This is the canonical search-box repro.
    with_test_owner(|| {
        let query = RwSignal::new(String::new());
        let r = resource::<String, _, _>(move || {
            // Synchronous read of `query` — registers as a dependency
            // of the resource's driving effect.
            let q = query.get();
            async move {
                if q.trim().is_empty() {
                    Ok(String::new())
                } else {
                    // A trivial "fetch": echo the query back uppercased.
                    Ok(q.to_uppercase())
                }
            }
        });

        // Initial fetch: empty query → empty result.
        tasks::run_until_stalled();
        assert_eq!(r.get().as_deref(), Some(""));

        // Change the tracked signal → effect re-runs → fetcher re-fires.
        query.set("Slime".to_string());
        flush(); // drain the effect re-run (spawns the new fetch)
        tasks::run_until_stalled(); // drive the new fetch to completion
        assert_eq!(
            r.get().as_deref(),
            Some("SLIME"),
            "resource must re-fetch with the new query after sync-prefix read changes"
        );

        // And again, to prove it keeps tracking across re-runs.
        query.set("Goo".to_string());
        flush();
        tasks::run_until_stalled();
        assert_eq!(r.get().as_deref(), Some("GOO"));
    });
}

#[test]
fn async_resource_tracks_signal_read_after_await_and_refetches() {
    // (b) After-await tracking: the fetcher first `.await`s a future
    // that is Pending on its first poll (a real suspension), and only
    // AFTER resuming reads `multiplier`. The signal read therefore
    // happens while the future is being polled outside the scheduler
    // run — it is the per-poll `with_observer` re-install that makes it
    // a tracked dependency. Changing `multiplier` must re-fetch.
    use std::pin::Pin;
    use std::task::{Context, Poll};

    // Ready-on-second-poll future, forcing a suspension before the read.
    struct OneYield(bool);
    impl std::future::Future for OneYield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if !self.0 {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    with_test_owner(|| {
        let multiplier = RwSignal::new(2_i32);
        let r = resource::<i32, _, _>(move || async move {
            // Suspend FIRST — the `multiplier` read below happens after
            // a real `.await` point, outside any scheduler run.
            OneYield(false).await;
            let m = multiplier.get();
            Ok(m * 10)
        });

        tasks::run_until_stalled();
        assert_eq!(
            r.get(),
            Some(20),
            "first fetch should suspend, resume, read multiplier=2 → 20"
        );

        // Change the post-await-tracked signal → must re-fetch.
        multiplier.set(5);
        flush();
        tasks::run_until_stalled();
        assert_eq!(
            r.get(),
            Some(50),
            "resource must track a signal read AFTER an .await and re-fetch on change"
        );
    });
}

#[test]
fn async_resource_returns_to_loading_during_refetch() {
    // A re-fetch whose future suspends should leave the resource in
    // Loading until the new run resolves (not stuck on the old value).
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct OneYield(bool);
    impl std::future::Future for OneYield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if !self.0 {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    with_test_owner(|| {
        let n = RwSignal::new(1_i32);
        let r = resource::<i32, _, _>(move || {
            let v = n.get();
            async move {
                OneYield(false).await;
                Ok(v)
            }
        });
        tasks::run_until_stalled();
        assert_eq!(r.get(), Some(1));

        // Re-run the effect (spawns a new, suspending fetch) but do NOT
        // drive the pool yet. The untracked Loading reset should be
        // visible immediately after flush.
        n.set(2);
        flush();
        assert!(
            r.loading(),
            "resource should be Loading during an in-flight re-fetch"
        );
        tasks::run_until_stalled();
        assert_eq!(r.get(), Some(2));
    });
}

/// Regression repro for the field bug on 0.2.4: a **reactive resource
/// whose fetcher reads a signal AND does blocking IO via `run_blocking`**
/// hangs after the tracked signal changes — the re-fetch starts
/// (`loading` flips true) but the result never lands.
///
/// ## Why this reproduces the device hang (and earlier attempts didn't)
///
/// On device the host render loop pauses whenever `whisker_tick`
/// reports **idle**, and resumes only on a `request_frame` wake. The
/// interim "busy-tick" fix kept the host ticking while tasks were
/// outstanding; the PROPER fix (modelled here) drives the parked fetch
/// off the **main run loop** instead.
///
/// A `run_blocking` fetch's result is marshaled back via the host's
/// main-thread dispatch (`run_on_main_thread` → CFRunLoop / Looper
/// post). The OS services that post even while the vsync render loop is
/// paused, and it is race-free (kernel-level wake). The `trampoline`
/// runs ON the main thread and, instead of requesting a (paused,
/// clobberable) vsync frame, invokes the registered DRIVE callback —
/// which runs a `tick_frame` equivalent there: drain the pool + flush,
/// completing and committing the fetch immediately.
///
/// ## How this harness faithfully models the worker→main-thread post
///
/// `run_blocking` calls `run_on_main_thread(|| tx.send(value))` from
/// the WORKER thread. The real host dispatch POSTS that to the MAIN
/// thread; the trampoline (and thus the drive) must run on the TEST
/// thread where the `POOL`/runtime thread-locals live — NOT inline on
/// the worker. So the test dispatcher ENQUEUES `(callback, user_data)`
/// into a thread-safe queue ([`POSTS`]); the test's host loop drains
/// that queue on the test thread each iteration, invoking the
/// trampoline there, which fires the test DRIVE callback (a stand-in
/// for the driver's `tick_frame`).
///
/// The frame-request callback is a NO-OP (the clobbered / lost vsync
/// wake): the ONLY thing that can finish a parked fetch is the
/// main-thread-posted trampoline → drive. The idle verdict is purely
/// `!dispatch_pending` (NO busy-tick). With the fix (trampoline drives)
/// the re-fetch COMPLETES; revert step 1 (trampoline back to
/// `wake_runtime` / no-op) and the drive never runs → the fetch is
/// never re-polled → these tests hit the deadline (the field hang).
mod cross_thread_wake {
    use super::*;
    use crate::main_thread::{set_drive_callback, set_main_thread_dispatcher, DispatchFn};
    use crate::tasks::run_blocking;
    use std::ffi::c_void;
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    // These tests reach into process-global state (the main-thread
    // dispatcher + drive callback + frame-request callback). Use the
    // shared host-test lock so sibling modules can't clear our wiring
    // mid-fetch.
    fn lock<'a>() -> MutexGuard<'a, ()> {
        crate::main_thread::host_test_lock()
    }

    /// A `(callback, user_data)` pair the dispatcher posted to the main
    /// thread. `user_data` is a raw `*mut c_void`; the host loop drains
    /// this on the test thread and invokes `callback(user_data)` — the
    /// trampoline — there.
    struct Post {
        callback: extern "C" fn(*mut c_void),
        user_data: *mut c_void,
    }
    // SAFETY: `user_data` is a `Box<Box<dyn FnOnce + Send>>` raw pointer
    // minted by `run_on_main_thread`; its closure is `Send`, so moving
    // the pointer to the test thread to invoke the trampoline is sound
    // (this is exactly what the real Lynx dispatch does).
    unsafe impl Send for Post {}

    // The main-thread post queue. The (worker-thread) dispatcher pushes;
    // the test's host loop drains on the test thread. Process-global but
    // serialized by `host_test_lock`; cleared in `install_host`.
    static POSTS: Mutex<Vec<Post>> = Mutex::new(Vec::new());

    // The host's frame-request callback, modelled as a NO-OP: this is
    // the *clobbered / lost* cross-thread vsync wake. A robust runtime
    // must NOT depend on it to finish an in-flight fetch.
    extern "C" fn request_frame_noop(_: *mut c_void) {}

    // FAITHFUL main-thread dispatcher: ENQUEUES the marshaled
    // (callback, user_data) for the MAIN (test) thread to run, rather
    // than invoking it inline on the caller (worker) thread. This is
    // what makes the trampoline — and the drive it triggers — run where
    // the runtime thread-locals live, exactly as the real Lynx
    // main-thread dispatch does.
    extern "C" fn enqueue_post(
        _engine: *mut c_void,
        callback: extern "C" fn(*mut c_void),
        user_data: *mut c_void,
    ) -> bool {
        if let Ok(mut q) = POSTS.lock() {
            q.push(Post {
                callback,
                user_data,
            });
        }
        true
    }

    // The TEST drive callback — stand-in for the driver's `tick_frame`
    // (the runtime crate can't call the driver). Runs the same shape:
    // reactive flush, drain the task pool, reactive flush (to surface
    // signal writes the drained fetch made, e.g. `state.set(Ready)`).
    // Invoked by the trampoline on the test thread.
    extern "C" fn test_drive() {
        flush();
        tasks::run_until_stalled();
        flush();
    }

    fn install_host() {
        if let Ok(mut q) = POSTS.lock() {
            q.clear();
        }
        set_main_thread_dispatcher(Some(enqueue_post as DispatchFn), std::ptr::null_mut());
        set_drive_callback(Some(test_drive));
        crate::host_wake::set_request_frame_callback(
            Some(request_frame_noop),
            std::ptr::null_mut(),
        );
    }

    fn reset_host() {
        crate::main_thread::__reset_for_tests();
        crate::host_wake::__reset_for_tests();
        if let Ok(mut q) = POSTS.lock() {
            q.clear();
        }
    }

    /// Drain every queued main-thread post on the TEST thread, invoking
    /// each trampoline (which fires the drive callback). Returns true if
    /// any post was drained — i.e. the main run loop did work this
    /// iteration. Models the OS servicing the main-loop queue even while
    /// the vsync loop is paused.
    fn drain_posts() -> bool {
        // Take the current batch out so a drive callback re-posting (a
        // chained fetch) lands in a fresh batch for the next iteration,
        // not an infinite inner loop.
        let batch: Vec<Post> = match POSTS.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(_) => Vec::new(),
        };
        let drained = !batch.is_empty();
        for post in batch {
            (post.callback)(post.user_data);
        }
        drained
    }

    /// One "frame": exactly what the driver's `tick_frame` +
    /// `tick`-idle-reporting do. Runs reactive flush, drains the task
    /// pool, runs a SECOND reactive flush (to surface signal writes
    /// made by tasks that resolved during the drain), and returns the
    /// runtime's idle/busy verdict — `true` == idle == "host may pause".
    ///
    /// The idle verdict mirrors `whisker-driver`'s reverted `tick`:
    /// purely `!dispatch_pending`. There is NO busy-tick — a parked
    /// fetch does NOT keep this returning busy. Modeled here as: a frame
    /// always completes its dispatched work synchronously, so it's
    /// always idle afterward.
    fn tick() -> bool {
        flush();
        tasks::run_until_stalled();
        flush();
        true // dispatch completed; idle. NO `!has_pending_tasks()`.
    }

    /// Drive the host loop until `done()` or the deadline.
    ///
    /// Models the real render loop: tick once; on idle the vsync loop
    /// "pauses" (stops ticking). Because the frame-request callback is
    /// the clobbered no-op, vsync can NOT resume a parked fetch. The
    /// ONLY resume path is the MAIN-THREAD POST queue: each loop
    /// iteration we [`drain_posts`] (the OS servicing CFRunLoop / Looper
    /// while vsync sleeps), which runs the trampoline → drive →
    /// `tick_frame`, completing the fetch. With step 1's fix the
    /// trampoline drives, so draining a post finishes the work; revert
    /// it (trampoline → `wake_runtime`/no-op) and draining a post does
    /// nothing useful → the fetch never re-polls → deadline → hang.
    fn drive_until(mut done: impl FnMut() -> bool) {
        // 2s is ample margin over the worker sleeps (15–40ms) on the
        // happy path; on a reverted fix the main-loop drive is inert and
        // the loop burns the full deadline (the hang).
        let deadline = Instant::now() + Duration::from_secs(2);

        // The vsync side ticks ONLY while it's busy. The moment it
        // reports idle it "pauses" and we NEVER tick it again from here
        // — exactly as the host pauses CADisplayLink/Choreographer. From
        // that point the ONLY thing that can advance the runtime is a
        // MAIN-THREAD POST being drained (the trampoline → drive). This
        // is what isolates the fix: with the proper fix the drained
        // post's trampoline runs `tick_frame` (completing the fetch);
        // reverted, the trampoline only fires the no-op vsync wake, the
        // value is delivered to the channel but NOTHING re-polls the pool
        // → deadline → hang.
        let mut vsync_idle = false;
        while !done() && Instant::now() < deadline {
            if !vsync_idle {
                // Vsync loop is running: tick frames until it idles. The
                // worker's blocking sleep means the fetch is still
                // Pending here, so this idles after kicking the fetch.
                vsync_idle = tick();
                std::thread::sleep(Duration::from_millis(1));
            } else {
                // Vsync paused. Service ONLY the main run loop (the OS
                // does this while vsync sleeps). Draining a post invokes
                // the trampoline → drive; that is the sole resume path.
                // We do NOT tick the vsync side here.
                drain_posts();
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    #[test]
    fn reactive_resource_reading_signal_with_run_blocking_completes_after_refetch() {
        use std::cell::Cell;
        use std::rc::Rc;

        let _g = lock();
        __reset_for_tests();
        tasks::__reset_for_tests();
        install_host();

        let runs = Rc::new(Cell::new(0u32));
        let runs_for_fetcher = runs.clone();

        let owner = Owner::new(None);
        owner.with(|| {
            let query = RwSignal::new(1_i32);
            let r = resource::<i32, _, _>(move || {
                runs_for_fetcher.set(runs_for_fetcher.get() + 1);
                // Sync-prefix read of the tracked signal (the canonical
                // "fetch keyed by a signal" pattern).
                let q = query.get();
                async move {
                    // Blocking IO offloaded to a worker thread, result
                    // marshaled back via run_on_main_thread. A small
                    // sleep makes the worker outlive the spawning frame
                    // so the cross-thread wake (not an inline poll) is
                    // what resumes the fetch — as on device.
                    let v = run_blocking(move || {
                        std::thread::sleep(Duration::from_millis(15));
                        q * 10
                    })
                    .await;
                    Ok::<i32, String>(v)
                }
            });

            // A consuming effect, as a component's render would have:
            // it READS the resource state (subscribing to the state
            // signal) so that each commit (`state.set`) schedules a
            // re-run on the trailing flush — faithfully modelling the
            // device, where the resource is rendered by `{expr}` text
            // bindings.
            let seen = Rc::new(Cell::new(0i32));
            let seen_for_effect = seen.clone();
            crate::reactive::effect::effect(move || {
                if let ResourceState::Ready(v) = r.state() {
                    seen_for_effect.set(v);
                }
            });

            // Drive the INITIAL fetch to completion.
            drive_until(|| !r.loading());
            assert_eq!(
                r.get(),
                Some(10),
                "initial reactive+run_blocking fetch must complete (query=1 → 10)"
            );

            // Change the tracked signal. On device the write's wake
            // requests a frame; the host tick flushes the effect re-run
            // (which spawns the new fetch). Do NOT flush inline — let
            // the on-demand tick loop pick up the frame request, exactly
            // as the host does.
            query.set(7);

            // Drive the RE-FETCH to completion. THIS is what hangs on
            // 0.2.4: loading flips true but the result never arrives.
            drive_until(|| matches!(r.state(), ResourceState::Ready(70)));
            assert_eq!(
                r.get(),
                Some(70),
                "reactive resource + run_blocking must finish the re-fetch \
                 after the tracked signal changes (query=7 → 70), not stay Loading"
            );
            assert_eq!(
                runs.get(),
                2,
                "fetcher must run exactly twice (initial + one re-fetch); a \
                 spurious extra run would bump the generation and abandon the \
                 in-flight fetch"
            );
            assert_eq!(seen.get(), 70, "consumer effect observed the final value");
        });

        owner.dispose();
        reset_host();
    }

    /// Reporter's after-`.await` variant: the tracked signal is read
    /// AFTER the `run_blocking().await` resumes — so the read happens
    /// during a cross-thread-woken poll, inside `with_observer`. This
    /// is the path the reporter's hypothesis points at (re-subscription
    /// during the cross-thread-woken poll).
    #[test]
    fn reactive_resource_signal_read_after_run_blocking_await_refetches() {
        let _g = lock();
        __reset_for_tests();
        tasks::__reset_for_tests();
        install_host();

        let owner = Owner::new(None);
        owner.with(|| {
            let multiplier = RwSignal::new(2_i32);
            let r = resource::<i32, _, _>(move || async move {
                // Blocking IO FIRST (a real cross-thread suspension),
                // then read the tracked signal AFTER resuming.
                let base = run_blocking(|| {
                    std::thread::sleep(Duration::from_millis(15));
                    10_i32
                })
                .await;
                let m = multiplier.get();
                Ok::<i32, String>(base * m)
            });

            drive_until(|| !r.loading());
            assert_eq!(r.get(), Some(20), "initial: 10 * multiplier(2) = 20");

            multiplier.set(5);
            drive_until(|| matches!(r.state(), ResourceState::Ready(50)));
            assert_eq!(
                r.get(),
                Some(50),
                "after-await tracked read + run_blocking must re-fetch (10*5=50)"
            );
        });

        owner.dispose();
        reset_host();
    }

    /// Overlapping re-fetch: the tracked signal changes WHILE the first
    /// fetch's worker is still in flight. This leaves a stale (gen-1)
    /// ScopedFetch in the pool that must be abandoned, and a fresh
    /// (gen-2) ScopedFetch that must run to completion. The bug: the
    /// stale task's cross-thread wake (its worker finishes later) gets
    /// tangled with the live task so the live result never lands.
    #[test]
    fn reactive_resource_overlapping_refetch_completes_latest() {
        use std::cell::Cell;
        use std::rc::Rc;

        let _g = lock();
        __reset_for_tests();
        tasks::__reset_for_tests();
        install_host();

        let owner = Owner::new(None);
        owner.with(|| {
            let query = RwSignal::new(1_i32);
            let r = resource::<i32, _, _>(move || {
                let q = query.get();
                async move {
                    let v = run_blocking(move || {
                        // Long enough that the second change lands while
                        // this worker is still asleep.
                        std::thread::sleep(Duration::from_millis(40));
                        q * 10
                    })
                    .await;
                    Ok::<i32, String>(v)
                }
            });

            let seen = Rc::new(Cell::new(0i32));
            let seen_for_effect = seen.clone();
            crate::reactive::effect::effect(move || {
                if let ResourceState::Ready(v) = r.state() {
                    seen_for_effect.set(v);
                }
            });

            // Kick the first fetch (do NOT wait for it).
            tick();
            assert!(r.loading());

            // Change the signal while the first worker is still asleep.
            query.set(7);

            // Drive to the LATEST result.
            drive_until(|| matches!(r.state(), ResourceState::Ready(70)));
            assert_eq!(
                r.get(),
                Some(70),
                "overlapping re-fetch must settle on the latest query (7 → 70), \
                 not hang in Loading"
            );
        });

        owner.dispose();
        reset_host();
    }
}

#[test]
fn resource_state_helpers_match_active_branch() {
    let loading: ResourceState<i32> = ResourceState::Loading;
    assert!(loading.is_loading());
    assert!(!loading.is_ready());
    assert!(!loading.is_error());

    let ready: ResourceState<i32> = ResourceState::Ready(1);
    assert!(!ready.is_loading());
    assert!(ready.is_ready());
    assert!(!ready.is_error());

    let err: ResourceState<i32> = ResourceState::Error("x".into());
    assert!(!err.is_loading());
    assert!(!err.is_ready());
    assert!(err.is_error());
}
