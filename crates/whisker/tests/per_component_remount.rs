//! Tests for true per-component remount.
//!
//! Direct invocation of `remount_components_for(&[fn_ptr])` simulates
//! the subsecond patch-applied hook in `whisker-driver::bootstrap`.
//! The component fn body itself isn't actually swapped by these
//! tests (subsecond is a runtime patch system; tests don't load
//! dylibs), but the remount machinery — dispose old owner, re-run
//! body, swap element subtree under the same parent slot — is fully
//! exercised.
//!
//! The wrapper-less model (issue #17): `#[component]` returns its
//! body root directly. The runtime captures `(parent, anchor)` from
//! the next `view::append_child` after the component fn returns, so
//! tests need to attach the result to some parent before invoking
//! `remount_components_for`.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{
    __reset_for_tests, create_owner, dispose_owner, owners_for_fn, remount_components_for,
    with_owner,
};
use whisker::runtime::view::{
    __reset_children_mirror_for_tests, append_child, create_element, install_renderer,
    uninstall_renderer, DynRenderer, ElementHandle,
};
use whisker::{flush, ElementTag};
use whisker::runtime::reactive::__reset_pending_mount_for_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
    Remove { parent: u32, child: u32 },
    Event { id: u32, name: String },
    Release { id: u32 },
}

#[derive(Default)]
struct Recorder {
    next: u32,
    log: Rc<RefCell<Vec<Op>>>,
}

impl Recorder {
    fn new() -> (Self, Rc<RefCell<Vec<Op>>>) {
        let r = Self::default();
        let log = r.log.clone();
        (r, log)
    }
}

impl DynRenderer for Recorder {
    fn create_element(&mut self, tag: ElementTag) -> ElementHandle {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::Create { id, tag });
        ElementHandle::from_raw(id)
    }
    fn release_element(&mut self, h: ElementHandle) {
        self.log.borrow_mut().push(Op::Release { id: h.id() });
    }
    fn set_attribute(&mut self, h: ElementHandle, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_inline_styles(&mut self, h: ElementHandle, css: &str) {
        self.log.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&mut self, p: ElementHandle, c: ElementHandle) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&mut self, p: ElementHandle, c: ElementHandle) {
        self.log.borrow_mut().push(Op::Remove {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn set_event_listener(
        &mut self,
        h: ElementHandle,
        name: &str,
        _cb: Box<dyn Fn() + 'static>,
    ) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&mut self, _p: ElementHandle) {}
    fn flush(&mut self) {}
}

/// Reset every thread-local the per-component remount machinery
/// touches so tests don't leak state across runs (when sharing a
/// thread, e.g. `--test-threads=1`).
fn reset_state() {
    __reset_for_tests();
    __reset_pending_mount_for_tests();
    __reset_children_mirror_for_tests();
}

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    reset_state();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);
    let result = with_owner(owner, || f(log));
    dispose_owner(owner);
    uninstall_renderer(None);
    result
}

// ----------------------------------------------------------------------------
// Component fixtures
// ----------------------------------------------------------------------------

#[component]
fn leaf(label: &'static str) -> ElementHandle {
    render! {
        view {
            text { {label} }
        }
    }
}

/// Attach a component to a fresh test parent element and return both
/// handles. The "parent" stands in for whatever element the user's
/// `render!` would have appended the component to in real code; the
/// MountSite's `parent` / `anchor` get bound the moment we
/// `append_child` here.
fn mount_under_test_parent(make: impl FnOnce() -> ElementHandle) -> (ElementHandle, ElementHandle) {
    let parent = create_element(ElementTag::View);
    let root = make();
    append_child(parent, root);
    (parent, root)
}

// ----------------------------------------------------------------------------
// Basic shape
// ----------------------------------------------------------------------------

#[test]
fn component_returns_body_root_directly() {
    with_recorder_and_owner(|log| {
        let _root = leaf("hello");
        let creates: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Create { id, tag } => Some((*id, *tag)),
                _ => None,
            })
            .collect();
        // First created element should be the body's outer `view`
        // (id 0), not a separate wrapper. The text and raw_text
        // follow.
        assert!(
            !creates.is_empty(),
            "leaf must create at least one element"
        );
        assert_eq!(
            creates[0],
            (0, ElementTag::View),
            "wrapper-less mount: first element is the body's view, not a wrapper"
        );
    });
}

#[test]
fn remount_replaces_root_at_same_parent_slot() {
    with_recorder_and_owner(|log| {
        let (parent, root_initial) = mount_under_test_parent(|| leaf("v1"));
        log.borrow_mut().clear();

        // Simulate a subsecond patch on `leaf`'s fn pointer.
        remount_components_for(&[leaf as *const ()]);

        let ops = log.borrow();
        // The old body root was removed from the test parent.
        assert!(
            ops.iter().any(|op| matches!(
                op,
                Op::Remove { parent: p, child: c }
                    if *p == parent.id() && *c == root_initial.id()
            )),
            "old body root removed from parent; ops were {ops:?}"
        );
        // A fresh `view` was created for the new body.
        assert!(
            ops.iter()
                .any(|op| matches!(op, Op::Create { tag, .. } if *tag == ElementTag::View)),
            "new body's view created; ops were {ops:?}"
        );
        // The label re-renders to the same value (subsecond didn't
        // actually swap the body; we just re-invoke it).
        assert!(
            ops.iter().any(|op| matches!(
                op,
                Op::SetAttr { key, value, .. }
                    if key == "text" && value == "v1"
            )),
            "new body re-rendered the same label; ops were {ops:?}"
        );
        // The new root was attached under the *same* parent.
        assert!(
            ops.iter().any(|op| matches!(
                op,
                Op::Append { parent: p, .. } if *p == parent.id()
            )),
            "new body attached to same parent; ops were {ops:?}"
        );
    });
}

#[test]
fn remount_disposes_old_owner_and_registers_new() {
    with_recorder_and_owner(|_log| {
        let (_parent, _root) = mount_under_test_parent(|| leaf("first"));
        let initial_owners = owners_for_fn(leaf as *const ());
        assert_eq!(initial_owners.len(), 1);
        let first_owner_id = initial_owners[0];

        remount_components_for(&[leaf as *const ()]);

        let after_owners = owners_for_fn(leaf as *const ());
        assert_eq!(after_owners.len(), 1);
        assert_ne!(
            after_owners[0], first_owner_id,
            "remount must create a fresh owner (different OwnerId)"
        );
    });
}

// ----------------------------------------------------------------------------
// Element-handle leak coverage — pins down the cleanup on dispose.
// ----------------------------------------------------------------------------

#[test]
fn remount_releases_old_body_elements() {
    with_recorder_and_owner(|log| {
        let (_parent, _root) = mount_under_test_parent(|| leaf("v1"));
        // Count elements created by the component itself (everything
        // the test parent's setup added is also in the log; we just
        // count Creates from after our parent's creation).
        let creates_initial_total = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::Create { .. }))
            .count();
        // The test parent is the first Create; the rest belong to
        // the component.
        let component_elements = creates_initial_total - 1;

        log.borrow_mut().clear();
        remount_components_for(&[leaf as *const ()]);

        let releases = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::Release { .. }))
            .count();
        assert_eq!(
            releases, component_elements,
            "every element under the disposed component owner must be released; \
             expected {component_elements} (component's element count), got {releases}",
        );
    });
}

#[test]
fn dispose_owner_releases_owned_elements() {
    // A leaf component mount creates several elements through
    // `view::*` while the component's owner is live. Disposing the
    // outer owner (e.g. when `<Show>` flips false, `<For>` removes
    // an item) cascades through the component owner and must
    // release every element it tracked.
    reset_state();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));

    let root_owner = create_owner(None);
    // Mount the component inside the root owner.
    let _root = with_owner(root_owner, || leaf("hi"));

    let creates_initial = log
        .borrow()
        .iter()
        .filter(|op| matches!(op, Op::Create { .. }))
        .count();
    log.borrow_mut().clear();

    dispose_owner(root_owner);

    let releases = log
        .borrow()
        .iter()
        .filter(|op| matches!(op, Op::Release { .. }))
        .count();
    assert_eq!(
        releases, creates_initial,
        "every element created under the disposed owner tree must be released; \
         expected {creates_initial}, got {releases}",
    );

    uninstall_renderer(None);
}

#[test]
fn remount_preserves_signal_held_above_in_context() {
    // Demonstrates the recommended state-survival pattern: state
    // held in an outer signal/context survives remount, while
    // state local to the remounted component is lost.

    #[derive(Copy, Clone)]
    struct AppState {
        counter: RwSignal<i32>,
    }

    #[component]
    fn inner_screen() -> ElementHandle {
        let state = use_context::<AppState>().unwrap();
        let local = signal(99_i32);
        render! {
            view {
                text { {state.counter.get()} }
                text { {local.0.get()} }
            }
        }
    }

    with_recorder_and_owner(|log| {
        provide_context(AppState {
            counter: RwSignal::new(42),
        });
        let (_parent, _root) = mount_under_test_parent(|| inner_screen());
        log.borrow_mut().clear();

        // Mutate the outer state, then remount.
        let state = use_context::<AppState>().unwrap();
        state.counter.set(100);
        remount_components_for(&[inner_screen as *const ()]);
        flush();

        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.contains(&"100".to_string()),
            "remounted component sees the outer context value; got texts {texts:?}"
        );
        assert!(
            texts.contains(&"99".to_string()),
            "local signal initialised to 99 in the new mount"
        );
    });
}
