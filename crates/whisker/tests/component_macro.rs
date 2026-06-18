//! Integration test for component mounting.
//!
//! Originally tested `#[component]` decorated functions with various
//! return types. After the True per-component remount change
//! (`#[component]` now always returns `Element` via the
//! remountable mount path), this file exercises the underlying
//! `mount_component` API directly to verify owner cascade /
//! cleanup semantics that the proc-macro is built on. The macro's
//! own remount behaviour is covered by a separate test that
//! installs a recording renderer.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{
    __reset_for_tests, mount_component, on_cleanup, owners_for_fn, unmount_component,
};

// Stable fn pointers for the tests. Using these instead of
// component fns since the macro now forces an `Element`
// return type and the owner-mechanic tests don't render anything.
fn dummy_outer() {}
fn dummy_inner() {}

#[test]
fn mount_component_runs_body_inside_owner() {
    __reset_for_tests();
    let sink = Rc::new(RefCell::new(0));
    let sink_clone = sink.clone();
    let (_owner, returned) = mount_component(dummy_outer as *const (), move || {
        let (count, _set_count) = signal(7_i32).split();
        let v = count.get();
        *sink_clone.borrow_mut() = v;
        v
    });
    assert_eq!(returned, 7);
    assert_eq!(*sink.borrow(), 7);

    let registered = owners_for_fn(dummy_outer as *const ());
    assert_eq!(registered.len(), 1);

    unmount_component(registered[0]);
    assert_eq!(owners_for_fn(dummy_outer as *const ()).len(), 0);
}

#[test]
fn nested_mounts_create_owner_tree() {
    __reset_for_tests();
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let log_outer = log.clone();
    let (_outer_owner, _) = mount_component(dummy_outer as *const (), move || {
        log_outer.borrow_mut().push("outer-enter");
        let log_inner = log_outer.clone();
        let (_inner_owner, _) = mount_component(dummy_inner as *const (), move || {
            log_inner.borrow_mut().push("inner-enter");
            let log_cleanup = log_inner.clone();
            on_cleanup(move || log_cleanup.borrow_mut().push("inner-cleanup"));
        });
        log_outer.borrow_mut().push("outer-exit");
    });

    // Body ran top-down.
    assert_eq!(
        *log.borrow(),
        vec!["outer-enter", "inner-enter", "outer-exit"]
    );

    let outer_owners = owners_for_fn(dummy_outer as *const ());
    let inner_owners = owners_for_fn(dummy_inner as *const ());
    assert_eq!(outer_owners.len(), 1);
    assert_eq!(inner_owners.len(), 1);

    // Disposing the outer owner cascades into the inner one.
    unmount_component(outer_owners[0]);
    assert_eq!(
        log.borrow().last().copied(),
        Some("inner-cleanup"),
        "inner cleanup must fire when outer is disposed"
    );
    assert_eq!(owners_for_fn(dummy_inner as *const ()).len(), 0);
}
