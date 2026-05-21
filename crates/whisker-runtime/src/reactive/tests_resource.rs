//! Unit tests for `resource_sync` (the synchronous variant; the
//! async `resource()` would require a registered main-thread
//! dispatcher and is exercised via the hn-reader integration test).

use crate::reactive::{__reset_for_tests, create_owner, resource_sync, with_owner, ResourceState};

fn with_test_owner<R>(f: impl FnOnce() -> R) -> R {
    __reset_for_tests();
    let owner = create_owner(None);
    with_owner(owner, f)
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
