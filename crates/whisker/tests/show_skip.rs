//! `Show` rebuilds its branch only when the condition **changes**, not
//! on every dependency change of `when`.
//!
//! `WriteSignal::set` notifies subscribers unconditionally (even for a
//! no-op write), so an effect keyed on a signal that `when` reads used
//! to tear down and re-mount the branch for an unchanged condition —
//! churning the DOM, disposing branch-internal state, and re-anchoring
//! following siblings. These tests pin the fixed behaviour.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::Owner;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, flush};
use whisker::runtime::view::{DynRenderer, Element, install_renderer, uninstall_renderer};

// ----- Recording renderer (records the mutations we care about) --------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32 },
    Append { parent: u32, child: u32 },
    Remove { parent: u32, child: u32 },
}

#[derive(Default)]
struct Recorder {
    next: std::cell::Cell<u32>,
    log: Rc<RefCell<Vec<Op>>>,
}

impl DynRenderer for Recorder {
    fn create_element(&self, _tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id });
        Element::from_raw(id)
    }
    fn release_element(&self, _h: Element) {}
    fn set_attribute(&self, _h: Element, _k: &str, _v: &str) {}
    fn set_inline_styles(&self, _h: Element, _css: &str) {}
    fn append_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Remove {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn set_event_listener(
        &self,
        _h: Element,
        _name: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
    }
    fn set_root(&self, _p: Element) {}
    fn flush(&self) {}
}

fn with_test_env<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
    let rec = Recorder::default();
    let log = rec.log.clone();
    let prev = install_renderer(Box::new(rec));
    let owner = Owner::new(None);
    let result = owner.with(|| f(log));
    uninstall_renderer(prev);
    result
}

#[test]
fn unchanged_cond_does_not_rebuild_branch() {
    with_test_env(|log| {
        let s = RwSignal::new(0i32);
        // The bool only flips at the threshold 10, so `s: 0 -> 1` leaves
        // the condition `true` while still notifying the effect.
        let _tree = render! {
            view() {
                Show(when: move || s.get() < 10) {
                    text(value: "branch")
                }
                text(value: "sibling")
            }
        };
        flush();

        // Everything above is the initial mount — ignore it.
        log.borrow_mut().clear();

        // Unchanged condition: the effect re-runs (it reads `s`), but the
        // branch must not be torn down / rebuilt, and the sibling must not
        // be re-anchored.
        s.set(1);
        flush();

        assert!(
            log.borrow().is_empty(),
            "unchanged condition must not mutate the tree, got: {:?}",
            log.borrow()
        );
    });
}

#[test]
fn changed_cond_still_rebuilds_branch() {
    with_test_env(|log| {
        let s = RwSignal::new(0i32);
        let _tree = render! {
            view() {
                Show(when: move || s.get() < 10) {
                    text(value: "branch")
                }
            }
        };
        flush();
        log.borrow_mut().clear();

        // Genuine flip true -> false: the branch must be torn down.
        s.set(20);
        flush();

        assert!(
            log.borrow()
                .iter()
                .any(|op| matches!(op, Op::Remove { .. })),
            "a real condition flip must still rebuild the branch, got: {:?}",
            log.borrow()
        );
    });
}
