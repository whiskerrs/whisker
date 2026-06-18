//! Integration tests for the `children()` slot in `render!`.
//!
//! `children()` lowers to
//! `whisker::runtime::view::mount_children(&children)` and mounts the
//! surrounding `#[component]`'s `children: Children` prop at the
//! current position via a phantom element. These tests verify:
//!
//! 1. The basic mount: children land at the slot position with
//!    siblings before / after preserved in order.
//! 2. Multi-projection: writing `children()` twice mounts the same
//!    children twice (the Rc is borrowed, not moved).
//! 3. Fragment children: a multi-element children block from the
//!    caller flattens correctly into the slot.
//! 4. FnMut re-invocation: the body can be invoked more than once
//!    (per-component remount / hot-reload) without `cannot move out
//!    of` errors — `mount_children` takes `&Children`, never moves.
//!
//! Naming convention matches `component_invocation.rs` (the longer
//! file that exercises every other Props-derived behaviour).

use std::cell::RefCell;
use std::rc::Rc;
use whisker::Owner;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, __reset_pending_mount_for_tests};
use whisker::runtime::view::{DynRenderer, Element, install_renderer, uninstall_renderer};

// ----- Recording renderer ----------------------------------------------------
//
// Same shape as `component_invocation.rs` — duplicated rather than
// shared because integration tests don't have a `tests/common`
// module convention yet, and the helper code is small.

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    SetAttr { id: u32, key: String, value: String },
    Append { parent: u32, child: u32 },
}

#[derive(Default)]
struct Recorder {
    next: ::std::cell::Cell<u32>,
    log: Rc<RefCell<Vec<Op>>>,
}

impl DynRenderer for Recorder {
    fn create_element(&self, tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create {
            id,
            tag: ElementTag::View,
        });
        Element::from_raw(id)
    }
    fn release_element(&self, _h: Element) {}
    fn set_attribute(&self, h: Element, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_inline_styles(&self, _h: Element, _css: &str) {}
    fn append_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&self, _p: Element, _c: Element) {}
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

fn fresh() {
    __reset_for_tests();
    __reset_pending_mount_for_tests();
}

fn with_test_env<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    fresh();
    let rec = Recorder::default();
    let log = rec.log.clone();
    let prev = install_renderer(Box::new(rec));
    let owner = Owner::new(None);
    let result = owner.with(|| f(log));
    uninstall_renderer(prev);
    result
}

// ----- Test components -------------------------------------------------------

/// A card with a fixed header text, a children slot, and a fixed
/// footer text. Used to verify sibling-ordering around the slot.
#[component]
fn card_with_slot(children: Children) -> Element {
    render! {
        view() {
            text(value: "header")
            children()
            text(value: "footer")
        }
    }
}

/// Mounts the same children twice. Verifies multi-projection works
/// — the Rc is borrowed, not moved, so the second `children()`
/// also resolves.
#[component]
fn double_mount(children: Children) -> Element {
    render! {
        view() {
            children()
            children()
        }
    }
}

/// `children()` at the very top — no sibling. The slot must still
/// mount the children inside the outer view.
#[component]
fn bare_slot(children: Children) -> Element {
    render! {
        view() {
            children()
        }
    }
}

// ----- Tests -----------------------------------------------------------------

#[test]
fn slot_mounts_children_between_siblings_in_order() {
    with_test_env(|log| {
        let _h = render! {
            CardWithSlot() {
                text(value: "slot-content")
            }
        };

        // Verify the three raw_text "text" attributes appear in the
        // expected order: header → slot → footer.
        let texts: Vec<String> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["header", "slot-content", "footer"],
            "slot content must land between header and footer"
        );
    });
}

#[test]
fn slot_mounts_multi_element_children_as_fragment() {
    with_test_env(|log| {
        // Caller passes two text elements inside the braces — the
        // macro routes them through a Fragment-returning closure.
        // The slot's `mount_children` flattens the fragment so both
        // texts end up at the slot position.
        let _h = render! {
            CardWithSlot() {
                text(value: "first")
                text(value: "second")
            }
        };

        let texts: Vec<String> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["header", "first", "second", "footer"],
            "fragment children must flatten in declaration order"
        );
    });
}

#[test]
fn slot_can_be_used_more_than_once() {
    with_test_env(|log| {
        // `double_mount` writes `children()` twice. The caller's
        // children closure is `Fn`, so invoking it twice produces
        // two independent renders of the same content. The
        // assertions count `raw_text` "text" attrs rather than
        // exact equality so we don't depend on phantom-element
        // ordering details.
        let _h = render! {
            DoubleMount() {
                text(value: "echo")
            }
        };

        let echoes = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::SetAttr { key, value, .. } if key == "text" && value == "echo"))
            .count();
        assert_eq!(echoes, 2, "children() called twice → two text creations");
    });
}

#[test]
fn slot_with_omitted_children_renders_nothing_extra() {
    with_test_env(|log| {
        // No `{ … }` block at the call site → children defaults
        // to a closure returning View::Empty. `mount_children`
        // still creates a phantom but attaches nothing to it, so
        // no raw_text elements get created.
        let _h = render! { BareSlot() };

        let raw_text_creates = log
            .borrow()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    Op::Create {
                        tag: ElementTag::RawText,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            raw_text_creates, 0,
            "empty children must not create any raw_text"
        );
    });
}

#[test]
fn slot_body_can_be_reinvoked() {
    // Per-component remount / hot-reload re-invoke the component's
    // outer `FnMut` body. `children()` lowers to a `&children`
    // borrow inside `mount_children`, so nothing moves and the
    // second invocation succeeds. We simulate the re-invocation by
    // constructing the Props once and calling the inner fn twice.
    with_test_env(|log| {
        use whisker::runtime::view::{Children, View};
        let children: Children = Rc::new(|| View::Text("echo".to_string()));

        let props1 = BareSlotProps::builder().children(children.clone()).build();
        let _h1 = BareSlot(props1);

        let props2 = BareSlotProps::builder().children(children.clone()).build();
        let _h2 = BareSlot(props2);

        // Each invocation should produce one raw_text("echo"). Two
        // invocations → two echoes. (If `mount_children` moved the
        // Rc, the second call would panic / fail to compile.)
        let echoes = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::SetAttr { key, value, .. } if key == "text" && value == "echo"))
            .count();
        assert_eq!(
            echoes, 2,
            "re-invoking the body must mount children both times"
        );
    });
}
