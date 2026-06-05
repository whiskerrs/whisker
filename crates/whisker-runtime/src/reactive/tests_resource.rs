//! Unit tests for `resource_sync` + the async `resource()`.
//!
//! `resource()` polls its `async` fetcher on the runtime's task pool
//! ([`crate::tasks`]); tests drive the pool with
//! [`crate::tasks::run_until_stalled`] to make the async path
//! observable without needing an active main-thread dispatcher (for
//! purely-async fetchers; `run_blocking`-using fetchers do require
//! the dispatcher and are exercised in `tasks::tests`).

use crate::reactive::{__reset_for_tests, resource, resource_sync, Owner, ResourceState};
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
