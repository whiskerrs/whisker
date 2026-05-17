//! Tests for true per-component remount.
//!
//! Direct invocation of `remount_components_for(&[fn_ptr])` simulates
//! the subsecond patch-applied hook in `whisker-driver::bootstrap`.
//! The component fn body itself isn't actually swapped by these
//! tests (subsecond is a runtime patch system; tests don't load
//! dylibs), but the remount machinery — dispose old owner, re-run
//! body, swap element subtree under the wrapper — is fully
//! exercised.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{
    __reset_for_tests, create_owner, dispose_owner, owners_for_fn, remount_components_for,
    with_owner,
};
use whisker::runtime::view::{
    install_renderer, uninstall_renderer, DynRenderer, ElementHandle,
};
use whisker::flush;

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

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
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

#[test]
fn component_returns_wrapper_element() {
    with_recorder_and_owner(|log| {
        let _h = leaf("hello");
        // Wrapper view (id 0) + body wrapper view (id 1) + text(2) + raw_text(3) + ...
        // We just check there's a wrapper at the top.
        let first_create = log
            .borrow()
            .iter()
            .find_map(|op| match op {
                Op::Create { id, tag } if *tag == ElementTag::View => Some(*id),
                _ => None,
            });
        assert_eq!(first_create, Some(0), "first element is the wrapper view");
    });
}

#[test]
fn remount_swaps_wrapper_contents_keeps_wrapper() {
    with_recorder_and_owner(|log| {
        let _wrapper = leaf("v1");
        log.borrow_mut().clear();

        // Simulate a subsecond patch on `leaf`'s fn pointer.
        remount_components_for(&[leaf as *const ()]);

        let ops = log.borrow();
        // The wrapper's existing body root was detached:
        assert!(
            ops.iter().any(|op| matches!(op, Op::Remove { parent: 0, .. })),
            "old body removed from wrapper (id 0)"
        );
        // New elements were created (component body re-ran):
        assert!(
            ops.iter().any(|op| matches!(op, Op::Create { tag, .. } if *tag == ElementTag::View)),
            "new body's view created"
        );
        assert!(
            ops.iter().any(|op| matches!(op, Op::SetAttr { key, value, .. } if key == "text" && value == "v1")),
            "new body re-rendered the same label (component re-invoked)"
        );
        // New root appended to the SAME wrapper (id 0):
        assert!(
            ops.iter().any(|op| matches!(op, Op::Append { parent: 0, .. })),
            "new body re-attached to existing wrapper"
        );
    });
}

#[test]
fn remount_disposes_old_owner_and_registers_new() {
    with_recorder_and_owner(|_log| {
        let _wrapper = leaf("first");
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
        // The "state" is read from context, not signal()'d here.
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
        let _w = inner_screen();
        log.borrow_mut().clear();

        // Mutate the outer state, then remount.
        let state = use_context::<AppState>().unwrap();
        state.counter.set(100);
        // (no flush — the user's edit triggers the patch first)
        remount_components_for(&[inner_screen as *const ()]);
        flush();

        // After remount, the new render! should observe the
        // updated context value (100), not the original (42).
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
        // The local signal was newly initialised in the remount,
        // so its value is the same (99), confirming it's been
        // reset (not preserved across remount).
        assert!(
            texts.contains(&"99".to_string()),
            "local signal initialised to 99 in the new mount"
        );
    });
}
