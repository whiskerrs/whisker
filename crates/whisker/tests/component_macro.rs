//! Integration test for the `#[component]` proc-macro.
//!
//! Compiles a tiny user-like crate against `whisker` and asserts the
//! mounting behaviour: the component fn ptr is registered against
//! the runtime, the body runs inside the new owner, the return type
//! is preserved, unmounting cleans up.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, owners_for_fn, unmount_component};

#[component]
fn counter(initial: i32, sink: Rc<RefCell<i32>>) -> i32 {
    let (count, _set_count) = signal(initial);
    *sink.borrow_mut() = count.get();
    count.get()
}

#[test]
fn component_macro_runs_body_inside_owner() {
    __reset_for_tests();
    let sink = Rc::new(RefCell::new(0));
    let returned = counter(7, sink.clone());
    assert_eq!(returned, 7);
    assert_eq!(*sink.borrow(), 7);

    // The fn pointer should be registered with at least one owner
    // (the one created by the macro on this call).
    let registered = owners_for_fn(counter as *const ());
    assert_eq!(registered.len(), 1);

    // Tear it down.
    unmount_component(registered[0]);
    assert_eq!(owners_for_fn(counter as *const ()).len(), 0);
}

#[component]
fn outer(log: Rc<RefCell<Vec<&'static str>>>) -> () {
    log.borrow_mut().push("outer-enter");
    let _ = inner(log.clone());
    log.borrow_mut().push("outer-exit");
}

#[component]
fn inner(log: Rc<RefCell<Vec<&'static str>>>) -> () {
    log.borrow_mut().push("inner-enter");
    on_cleanup(move || log.borrow_mut().push("inner-cleanup"));
}

#[test]
fn nested_components_create_owner_tree() {
    __reset_for_tests();
    let log = Rc::new(RefCell::new(Vec::new()));
    outer(log.clone());

    // Body ran top-down.
    assert_eq!(*log.borrow(), vec!["outer-enter", "inner-enter", "outer-exit"]);

    // Outer + inner each registered one owner.
    let outer_owners = owners_for_fn(outer as *const ());
    let inner_owners = owners_for_fn(inner as *const ());
    assert_eq!(outer_owners.len(), 1);
    assert_eq!(inner_owners.len(), 1);

    // Disposing the outer owner cascades into the inner one.
    unmount_component(outer_owners[0]);
    assert_eq!(
        log.borrow().last().copied(),
        Some("inner-cleanup"),
        "inner cleanup must fire when outer is disposed"
    );
    // Inner's registration is gone too.
    assert_eq!(owners_for_fn(inner as *const ()).len(), 0);
}
