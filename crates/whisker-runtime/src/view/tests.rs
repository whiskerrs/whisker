//! Tests for the view layer.
//!
//! Uses a small inline `RecordingRenderer` that records every
//! dispatched call into a `Vec<Op>`, which test assertions then
//! inspect.

use super::*;
use crate::element::ElementTag;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    Release { id: u32 },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
    Remove { parent: u32, child: u32 },
    Event { id: u32, name: String },
    SetRoot { id: u32 },
    Flush,
}

#[derive(Default)]
struct RecordingRenderer {
    // `Cell` because `DynRenderer` methods take `&self` now (the
    // re-entrancy fix): the renderer owns its mutable state behind
    // interior mutability instead of `&mut self`.
    next_id: std::cell::Cell<u32>,
    ops: std::rc::Rc<std::cell::RefCell<Vec<Op>>>,
}

impl RecordingRenderer {
    fn with_log() -> (Self, std::rc::Rc<std::cell::RefCell<Vec<Op>>>) {
        let renderer = Self::default();
        let log = renderer.ops.clone();
        (renderer, log)
    }

    fn alloc_id(&self) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }
}

impl DynRenderer for RecordingRenderer {
    fn create_element(&self, tag: ElementTag) -> Element {
        let id = self.alloc_id();
        self.ops.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.alloc_id();
        self.ops.borrow_mut().push(Op::Create {
            id,
            tag: ElementTag::View,
        });
        Element::from_raw(id)
    }
    fn release_element(&self, h: Element) {
        self.ops.borrow_mut().push(Op::Release { id: h.id() });
    }
    fn set_attribute(&self, h: Element, key: &str, value: &str) {
        self.ops.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: key.into(),
            value: value.into(),
        });
    }
    fn set_inline_styles(&self, h: Element, css: &str) {
        self.ops.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&self, parent: Element, child: Element) {
        self.ops.borrow_mut().push(Op::Append {
            parent: parent.id(),
            child: child.id(),
        });
    }
    fn remove_child(&self, parent: Element, child: Element) {
        self.ops.borrow_mut().push(Op::Remove {
            parent: parent.id(),
            child: child.id(),
        });
    }
    fn set_event_listener(
        &self,
        h: Element,
        name: &str,
        _bind_type: super::BindType,
        _callback: Box<dyn Fn(crate::value::WhiskerValue) + 'static>,
    ) {
        self.ops.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&self, page: Element) {
        self.ops.borrow_mut().push(Op::SetRoot { id: page.id() });
    }
    fn flush(&self) {
        self.ops.borrow_mut().push(Op::Flush);
    }
}

// ----- Renderer installation -----------------------------------------------

#[test]
fn dispatch_routes_to_installed_renderer() {
    let (renderer, log) = RecordingRenderer::with_log();
    let h = with_installed_renderer(Box::new(renderer), || {
        let h = create_element(ElementTag::View);
        set_attribute(h, "k", "v");
        flush();
        h
    });
    assert_eq!(h.id(), 0);
    let ops = log.borrow();
    assert_eq!(
        ops.as_slice(),
        &[
            Op::Create {
                id: 0,
                tag: ElementTag::View
            },
            Op::SetAttr {
                id: 0,
                key: "k".into(),
                value: "v".into()
            },
            Op::Flush,
        ],
    );
}

#[test]
fn dispatch_no_renderer_is_silent_noop() {
    // No installed renderer.
    let h = create_element(ElementTag::View);
    // Returns the sentinel `u32::MAX` and prints to stderr in debug.
    assert_eq!(h.id(), u32::MAX);
}

#[test]
fn install_returns_previous_renderer() {
    let (r1, _) = RecordingRenderer::with_log();
    let (r2, _) = RecordingRenderer::with_log();
    let prev = install_renderer(Box::new(r1));
    assert!(prev.is_none());
    let prev = install_renderer(Box::new(r2));
    assert!(prev.is_some());
    uninstall_renderer(prev);
    // Restore to first installed.
    let still_installed = current_renderer_id();
    assert!(still_installed.is_some());
    uninstall_renderer(None);
    assert!(current_renderer_id().is_none());
}

// ----- IntoView impls -------------------------------------------------------

#[test]
fn element_handle_into_view() {
    let h = Element::from_raw(7);
    let v = h.into_view();
    match v {
        View::Element(e) => assert_eq!(e.id(), 7),
        _ => panic!("expected Element"),
    }
}

#[test]
fn unit_into_empty() {
    let v: View = ().into_view();
    assert!(matches!(v, View::Empty));
    assert!(v.elements().is_empty());
}

#[test]
fn option_some_and_none() {
    let some_h = Some(Element::from_raw(3));
    let none_h: Option<Element> = None;
    assert_eq!(some_h.into_view().elements(), vec![Element::from_raw(3)]);
    assert_eq!(none_h.into_view().elements(), Vec::<Element>::new());
}

#[test]
fn tuple_into_fragment_preserves_order() {
    let v = (
        Element::from_raw(10),
        Element::from_raw(20),
        Element::from_raw(30),
    )
        .into_view();
    assert_eq!(
        v.elements(),
        vec![
            Element::from_raw(10),
            Element::from_raw(20),
            Element::from_raw(30),
        ]
    );
}

#[test]
fn nested_tuples_flatten_in_order() {
    let v = (
        Element::from_raw(1),
        (Element::from_raw(2), Element::from_raw(3)),
        Element::from_raw(4),
    )
        .into_view();
    assert_eq!(
        v.elements(),
        vec![
            Element::from_raw(1),
            Element::from_raw(2),
            Element::from_raw(3),
            Element::from_raw(4),
        ]
    );
}

#[test]
fn view_attach_appends_each_leaf() {
    let (renderer, log) = RecordingRenderer::with_log();
    with_installed_renderer(Box::new(renderer), || {
        let parent = create_element(ElementTag::View);
        let frag = View::Fragment(vec![
            View::Element(Element::from_raw(100)),
            View::Element(Element::from_raw(200)),
            View::Empty,
            View::Element(Element::from_raw(300)),
        ]);
        frag.attach_to(parent);
    });
    let ops = log.borrow();
    let appends: Vec<_> = ops
        .iter()
        .filter(|o| matches!(o, Op::Append { .. }))
        .collect();
    assert_eq!(appends.len(), 3, "Empty fragments must be skipped");
}

/// Reproduces the `Router { childA childB childC }` shape: a `Children`
/// closure builds a `View::Fragment` of several real children, attaches it
/// to a PHANTOM slot (`mount_children`), then that phantom is appended to a
/// REAL parent (`append_child(root, content)`). Every real child must reach
/// the renderer attached to the real root — the mirror's phantom-hoist must
/// not drop the 2nd..Nth child. (Pins the layer for the Android
/// "only the first Router child mounts" symptom: if this passes, the mirror
/// is correct and the divergence is renderer/bridge-side.)
#[test]
fn multi_child_fragment_through_phantom_slot_hoists_all_children() {
    let (renderer, log) = RecordingRenderer::with_log();
    let (root, c1, c2, c3) = with_installed_renderer(Box::new(renderer), || {
        let root = create_element(ElementTag::View); // real parent
        // The three real children a multi-child children block would build.
        let c1 = create_element(ElementTag::View);
        let c2 = create_element(ElementTag::View);
        let c3 = create_element(ElementTag::View);

        // `mount_children`: a phantom slot holding the Fragment.
        let slot = create_phantom_element();
        View::Fragment(vec![
            View::Element(c1),
            View::Element(c2),
            View::Element(c3),
        ])
        .attach_to(slot);

        // `append_child(root, content)` — content is the phantom slot.
        append_child(root, slot);
        (root, c1, c2, c3)
    });

    let ops = log.borrow();
    // Every child must have been appended to the REAL root.
    for (label, c) in [("c1", c1), ("c2", c2), ("c3", c3)] {
        assert!(
            ops.iter().any(|o| matches!(
                o,
                Op::Append { parent, child } if *parent == root.id() && *child == c.id()
            )),
            "{label} ({}) was never appended to the real root ({}); ops={ops:?}",
            c.id(),
            root.id()
        );
    }
}

// ===========================================================================
// Re-entrancy (whisker #3) — these tests pin the root-cause fix: a native
// event that fires *synchronously during* a renderer operation must be able
// to re-enter the dispatch path without aborting on "RefCell already
// borrowed". The fix is: `DynRenderer` methods take `&self`, renderers own
// their state behind interior `RefCell`s with FFI-scoped borrows, and
// `with_renderer` takes a *shared* borrow of the renderer slot.
// ===========================================================================
mod reentrancy {
    use super::*;
    use crate::value::WhiskerValue;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    /// A test renderer that mirrors the *shape* of the real
    /// `BridgeRenderer`: it keeps `parent_sign` and `listeners` behind
    /// per-field `RefCell`s and plans event dispatch from them. Its
    /// `remove_child` runs a caller-supplied **re-entrancy hook**
    /// *while simulating native teardown* — standing in for Lynx
    /// synchronously dispatching a UIKit event during `remove_child`.
    ///
    /// Crucially `remove_child` holds **no field borrow** across the
    /// hook (mirroring the FFI-scoping rule), so the hook is free to
    /// re-enter any `view::*` op, including `dispatch_event`, which
    /// reads `parent_sign` + `listeners`.
    #[derive(Default)]
    struct ReentrantRenderer {
        next_id: Cell<u32>,
        /// `element id` → its mirror parent id (used as the "sign" —
        /// the test models sign == element id for simplicity).
        parent_sign: RefCell<HashMap<i32, i32>>,
        /// `(sign, event)` → listener closures.
        #[allow(clippy::type_complexity)]
        listeners: RefCell<HashMap<(i32, String), Vec<Rc<dyn Fn(WhiskerValue)>>>>,
        /// Ordered side-effect log so tests can assert *sync* delivery
        /// order (outer op effects interleaved with inner re-entrant
        /// effects).
        log: Rc<RefCell<Vec<String>>>,
        /// Fired once, from inside `remove_child`, with no field borrow
        /// held — the simulated synchronous native callback.
        #[allow(clippy::type_complexity)]
        on_remove_hook: RefCell<Option<Box<dyn Fn()>>>,
    }

    impl ReentrantRenderer {
        fn new() -> (Self, Rc<RefCell<Vec<String>>>) {
            let r = Self::default();
            let log = r.log.clone();
            (r, log)
        }
        fn alloc_id(&self) -> u32 {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            id
        }
    }

    impl DynRenderer for ReentrantRenderer {
        fn create_element(&self, _tag: ElementTag) -> Element {
            Element::from_raw(self.alloc_id())
        }
        fn create_element_by_name(&self, _tag: &str) -> Element {
            Element::from_raw(self.alloc_id())
        }
        fn release_element(&self, _h: Element) {}
        fn element_sign(&self, h: Element) -> i32 {
            h.id() as i32
        }
        fn set_attribute(&self, h: Element, key: &str, value: &str) {
            self.log
                .borrow_mut()
                .push(format!("set_attr {} {}={}", h.id(), key, value));
        }
        fn set_inline_styles(&self, _h: Element, _css: &str) {}
        fn append_child(&self, parent: Element, child: Element) {
            // Scope the `parent_sign` borrow — never spanning anything
            // re-entrant (matches the renderer contract).
            self.parent_sign
                .borrow_mut()
                .insert(child.id() as i32, parent.id() as i32);
            self.log
                .borrow_mut()
                .push(format!("append {} -> {}", child.id(), parent.id()));
        }
        fn remove_child(&self, parent: Element, child: Element) {
            // Simulate native teardown: FIRST mutate `parent_sign`
            // under a short, scoped borrow that is fully released…
            {
                let mut ps = self.parent_sign.borrow_mut();
                ps.remove(&(child.id() as i32));
            }
            // …THEN run the synchronous "native callback" with NO field
            // borrow held. This is exactly the spot where Lynx would
            // re-enter Whisker during teardown. If any field borrow
            // leaked into here, the re-entrant op below would panic.
            self.log
                .borrow_mut()
                .push(format!("remove {} from {}", child.id(), parent.id()));
            let hook = self.on_remove_hook.borrow_mut().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        fn set_event_listener(
            &self,
            h: Element,
            event_name: &str,
            _bind_type: BindType,
            callback: Box<dyn Fn(WhiskerValue) + 'static>,
        ) {
            self.listeners
                .borrow_mut()
                .entry((h.id() as i32, event_name.to_string()))
                .or_default()
                .push(Rc::from(callback));
        }
        fn plan_event_dispatch(
            &self,
            target_sign: i32,
            event_name: &str,
            body: &WhiskerValue,
        ) -> EventDispatchPlan {
            // Reconstruct the chain under a scoped `parent_sign` borrow,
            // then drop it before touching `listeners` — mirroring the
            // real renderer. Both borrows are read-only and span no
            // re-entrant op.
            let chain = {
                let ps = self.parent_sign.borrow();
                let mut chain = vec![target_sign];
                let mut cur = target_sign;
                while let Some(&p) = ps.get(&cur) {
                    chain.push(p);
                    cur = p;
                }
                chain
            };
            let listeners = self.listeners.borrow();
            let mut firings: Vec<super::super::renderer::EventFiring> = Vec::new();
            // Bubble: target → root.
            for sign in &chain {
                if let Some(ls) = listeners.get(&(*sign, event_name.to_string())) {
                    for l in ls {
                        firings.push((l.clone(), body.clone()));
                    }
                }
            }
            EventDispatchPlan {
                consumed: !firings.is_empty(),
                firings,
            }
        }
        fn set_root(&self, _p: Element) {}
        fn flush(&self) {}
    }

    /// Re-entrant *renderer op* during a renderer op does not panic.
    /// `remove_child` synchronously calls back into `set_attribute`
    /// (another `with_renderer` shared borrow). Pre-fix this aborted
    /// with "already borrowed".
    #[test]
    fn reentrant_renderer_op_during_remove_does_not_panic() {
        let (renderer, log) = ReentrantRenderer::new();
        // Install a hook that re-enters the public dispatch path.
        *renderer.on_remove_hook.borrow_mut() = Some(Box::new(|| {
            // This goes through `with_renderer` again — a NESTED shared
            // borrow of CURRENT_RENDERER. Must be granted, not aborted.
            set_attribute(Element::from_raw(99), "reentrant", "1");
        }));

        with_installed_renderer(Box::new(renderer), || {
            let parent = create_element(ElementTag::View); // 0
            let child = create_element(ElementTag::View); // 1
            append_child(parent, child);
            // Triggers the hook mid-`remove_child`.
            remove_child(parent, child);
        });

        let log = log.borrow();
        assert!(
            log.iter().any(|l| l == "remove 1 from 0"),
            "outer remove ran: {log:?}"
        );
        assert!(
            log.iter().any(|l| l == "set_attr 99 reentrant=1"),
            "re-entrant set_attribute ran synchronously: {log:?}"
        );
        // The re-entrant op was observed AFTER the outer remove began —
        // synchronous, in-order, no deferral.
        let remove_pos = log.iter().position(|l| l == "remove 1 from 0").unwrap();
        let reentrant_pos = log
            .iter()
            .position(|l| l == "set_attr 99 reentrant=1")
            .unwrap();
        assert!(
            reentrant_pos > remove_pos,
            "re-entrant effect must come after the op that triggered it: {log:?}"
        );
    }

    /// Re-entrant *event dispatch* during a renderer op runs
    /// synchronously and in order. `remove_child` synchronously
    /// dispatches an event whose listener performs another renderer op
    /// — both the outer op and the inner listener's effect are observed
    /// in order. This proves there is no one-tick deferral (the #3
    /// `DispatchQueue.main.async` workaround is no longer needed).
    #[test]
    fn reentrant_event_dispatch_runs_synchronously_in_order() {
        let (renderer, log) = ReentrantRenderer::new();
        *renderer.on_remove_hook.borrow_mut() = Some(Box::new(|| {
            // Simulate the native reporter forwarding a custom event
            // during teardown.
            dispatch_event(7, "custominput", WhiskerValue::Null);
        }));

        with_installed_renderer(Box::new(renderer), || {
            let parent = create_element(ElementTag::View); // 0
            let child = create_element(ElementTag::View); // 1
            append_child(parent, child);

            // Register a listener on sign 7 that, WHEN FIRED, performs a
            // renderer op (set_attribute). Sign 7 is modeled as element
            // id 7.
            set_event_listener(
                Element::from_raw(7),
                "custominput",
                BindType::Bind,
                Box::new(|_v| {
                    set_attribute(Element::from_raw(42), "from-listener", "fired");
                }),
            );

            remove_child(parent, child);
        });

        let log = log.borrow();
        let remove_pos = log.iter().position(|l| l == "remove 1 from 0");
        let listener_pos = log
            .iter()
            .position(|l| l == "set_attr 42 from-listener=fired");
        assert!(remove_pos.is_some(), "outer remove ran: {log:?}");
        assert!(
            listener_pos.is_some(),
            "re-entrant event listener fired synchronously: {log:?}"
        );
        assert!(
            listener_pos.unwrap() > remove_pos.unwrap(),
            "listener effect must follow the triggering op, in order: {log:?}"
        );
    }

    /// Field-borrow scoping: the outer op is mid-mutation of the SAME
    /// field (`parent_sign`) that the re-entrant op reads/writes.
    /// `remove_child` mutates `parent_sign` (scoped + dropped) and then,
    /// in the hook, the re-entrant `dispatch_event` walks `parent_sign`
    /// AND a re-entrant `append_child` writes it. No panic; final state
    /// is correct. This guards against re-introducing a spanning borrow.
    #[test]
    fn field_borrow_scoping_same_field_reentrant() {
        let (renderer, log) = ReentrantRenderer::new();
        *renderer.on_remove_hook.borrow_mut() = Some(Box::new(|| {
            // READ parent_sign (chain walk) …
            dispatch_event(5, "tap", WhiskerValue::Null);
            // … and WRITE parent_sign (new edge), all re-entrantly while
            // the outer remove_child is on the stack.
            append_child(Element::from_raw(10), Element::from_raw(11));
        }));

        with_installed_renderer(Box::new(renderer), || {
            let p = create_element(ElementTag::View); // 0
            let a = create_element(ElementTag::View); // 1
            let b = create_element(ElementTag::View); // 2
            append_child(p, a);
            append_child(a, b); // parent_sign: 1->0, 2->1
            remove_child(a, b); // mutates parent_sign (removes 2->1), then hooks
        });

        let log = log.borrow();
        // Re-entrant append must have landed in parent_sign without a
        // borrow conflict.
        assert!(
            log.iter().any(|l| l == "append 11 -> 10"),
            "re-entrant append while outer remove on stack: {log:?}"
        );
        assert!(log.iter().any(|l| l == "remove 2 from 1"));
    }

    /// Nested `with_renderer` (shared borrow) is allowed, but a mut
    /// swap of the renderer slot *during* an outstanding shared borrow
    /// is still rejected — sanity that we didn't accidentally make the
    /// slot permissive to concurrent shared+exclusive access.
    #[test]
    fn mut_swap_during_dispatch_is_rejected() {
        let (renderer, _log) = ReentrantRenderer::new();
        // The hook attempts to swap the renderer slot (an exclusive
        // borrow) while the outer op holds a shared borrow → must
        // panic. We catch it so the test asserts on the rejection
        // rather than aborting.
        *renderer.on_remove_hook.borrow_mut() = Some(Box::new(|| {
            let result = std::panic::catch_unwind(|| {
                // `install_renderer` uses `with_borrow_mut`.
                let _ = install_renderer(Box::new(RecordingRenderer::default()));
            });
            assert!(
                result.is_err(),
                "swapping the renderer slot during a shared borrow must be rejected"
            );
        }));

        with_installed_renderer(Box::new(renderer), || {
            let parent = create_element(ElementTag::View);
            let child = create_element(ElementTag::View);
            append_child(parent, child);
            remove_child(parent, child);
        });
    }

    /// Nested shared `with_renderer` borrows stack arbitrarily deep
    /// without panicking — drives the re-entrant path two levels deep.
    #[test]
    fn deeply_nested_shared_borrows_do_not_panic() {
        let (renderer, log) = ReentrantRenderer::new();
        // Hook fires a renderer op which itself triggers another
        // renderer op via a listener → 3 stacked shared borrows.
        *renderer.on_remove_hook.borrow_mut() = Some(Box::new(|| {
            set_attribute(Element::from_raw(100), "level", "2");
            dispatch_event(8, "deep", WhiskerValue::Null);
        }));

        with_installed_renderer(Box::new(renderer), || {
            set_event_listener(
                Element::from_raw(8),
                "deep",
                BindType::Bind,
                Box::new(|_v| {
                    set_attribute(Element::from_raw(101), "level", "3");
                }),
            );
            let parent = create_element(ElementTag::View);
            let child = create_element(ElementTag::View);
            append_child(parent, child);
            remove_child(parent, child);
        });

        let log = log.borrow();
        assert!(log.iter().any(|l| l == "set_attr 100 level=2"));
        assert!(log.iter().any(|l| l == "set_attr 101 level=3"));
    }
}
