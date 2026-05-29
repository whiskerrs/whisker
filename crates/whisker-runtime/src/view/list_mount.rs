//! `list_mount` — the runtime driver for Lynx's `<list>` virtualisation
//! seen from Whisker.
//!
//! Drives an externally-created `<list>` Element by:
//!
//! 1. Subscribing to `items()` via an effect: whenever the data
//!    changes, the snapshot `current_items` is refreshed and Lynx is
//!    told the new item count via `set_update_list_info`.
//! 2. Installing a `NativeItemProvider`: Lynx's list machinery then
//!    calls `component_at_index(i)` on demand for each visible slot
//!    and `enqueue_component(sign)` when an item scrolls out. The
//!    provider builds (or releases) the FiberElement for that index
//!    using the user-supplied `body` closure, scoped to its own
//!    reactive owner so per-item state cleans up.
//!
//! On owner disposal (e.g. the surrounding component unmounts) every
//! live slot's owner is disposed and the provider is implicitly
//! released through `whisker-driver`'s `trampoline_free`.
//!
//! # API shape
//!
//! Mirrors [`for_each`](super::for_each) deliberately — `items` /
//! `key` / `body` with the same generic signatures — so users who
//! know one know both. The key difference is rendering: `for_each`
//! attaches every item to a wrapper view (no virtualisation), while
//! `list_mount` hands items to Lynx's `<list>` C++ machinery for
//! viewport-bounded virtualisation.
//!
//! # Limitations (v1)
//!
//! - **No per-item reactive recycling**: when an item enters the
//!   viewport its `body(item.clone())` runs fresh. Subsequent data
//!   updates to the *same key* don't re-bind the existing slot;
//!   instead, callers should change keys (or include a fingerprint
//!   in the key) to force a re-render. Per-item signals are tracked
//!   as P4-follow-up — they need a richer `update-list-info` diff
//!   schema from the bridge (Phase P or its `update-list-info`
//!   extension).
//! - **Diff is full-reset by item count only**: `update-list-info`
//!   is sent as "N items, item-keys `w_<i>`". Lynx's adapter re-binds
//!   visible slots correctly on `count` change, but the explicit
//!   `removeAction` / `moveAction` paths aren't exercised — `key` is
//!   accepted for API stability but not yet used by the diff.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::reactive::{create_owner, dispose_owner, effect, on_cleanup, with_owner, OwnerId};

use super::handle::Element;
use super::list_provider::{NativeItemProvider, INVALID_ITEM_INDEX};
use super::renderer::{install_list_native_item_provider, set_update_list_info};

/// One live item — its FiberElement (`element`) and the reactive
/// owner under which the body closure ran. Dropped when the slot is
/// enqueued out of viewport, or all-at-once on `list_mount`'s
/// `on_cleanup`.
struct Slot {
    #[allow(dead_code)] // kept for future "manual remove" code paths
    element: Element,
    owner: OwnerId,
}

/// Drive a Whisker-side virtualised list against an already-created
/// `<list>` Element. See module docs for the lifecycle.
///
/// - `handle` — the `<list>` Element the caller already created
///   (e.g. via the `list { … }` render! builder).
/// - `items` — reactive data source. Re-evaluated on every change of
///   any signal it reads.
/// - `key` — identity extractor. Accepted for API stability; unused
///   in v1 (see module-level "Limitations").
/// - `body` — slot builder. Called *once per slot creation* with a
///   clone of `items()[i]` and must return the slot's
///   `<list-item>` Element.
pub fn list_mount<T, K, ItemsFn, KeyFn, BodyFn>(
    handle: Element,
    items: ItemsFn,
    key: KeyFn,
    body: BodyFn,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
    ItemsFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    BodyFn: Fn(T) -> Element + 'static,
{
    let _ = key; // v1: unused, see module-level note.

    let current_items: Rc<RefCell<Vec<T>>> = Rc::new(RefCell::new(Vec::new()));
    let slots: Rc<RefCell<HashMap<i32, Slot>>> = Rc::new(RefCell::new(HashMap::new()));
    let body = Rc::new(body);

    // (1) Effect: items() → snapshot + count broadcast.
    {
        let current_items = current_items.clone();
        effect(move || {
            let new_items = items();
            let count = new_items.len() as i32;
            *current_items.borrow_mut() = new_items;
            // Tell Lynx the new count. The list will then call the
            // native provider for any newly-visible index and
            // `enqueue_component` for any slot whose index is no
            // longer represented.
            set_update_list_info(handle, count);
        });
    }

    // (2) Native item provider: pulls from `current_items` on demand.
    let provider = NativeItemProvider {
        component_at_index: {
            let current_items = current_items.clone();
            let slots = slots.clone();
            let body = body.clone();
            Box::new(move |index, _op_id, _reuse| {
                let item = match current_items.borrow().get(index as usize) {
                    Some(t) => t.clone(),
                    None => return INVALID_ITEM_INDEX,
                };
                // Each slot lives under its own owner so reactive
                // effects inside the body (signal subscriptions
                // etc.) get cleaned up cleanly when the slot is
                // enqueued out.
                let owner = create_owner(None);
                let element = with_owner(owner, || (body)(item));
                let sign = element.id() as i32;
                slots.borrow_mut().insert(sign, Slot { element, owner });
                sign
            })
        },
        enqueue_component: Some({
            let slots = slots.clone();
            Box::new(move |sign| {
                if let Some(slot) = slots.borrow_mut().remove(&sign) {
                    dispose_owner(slot.owner);
                }
            })
        }),
    };
    install_list_native_item_provider(handle, provider);

    // (3) Cleanup: on owner disposal (e.g. surrounding component
    // unmount) tear down any slots still alive. The provider itself
    // is released via the bridge's `trampoline_free` when the list
    // element dies, which drops the `Box<dyn FnMut>`s — but that
    // doesn't reach the per-slot owners, so we have to dispose them
    // here.
    on_cleanup(move || {
        let mut slots = slots.borrow_mut();
        for (_, slot) in slots.drain() {
            dispose_owner(slot.owner);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::ElementTag;
    use crate::reactive::{create_owner, dispose_owner, flush, with_owner};
    use crate::view::renderer::{install_renderer, uninstall_renderer, DynRenderer};
    use crate::view::{create_element_by_name, BindType};

    /// Minimal recording renderer that captures the calls
    /// `list_mount` makes — `set_update_list_info(count)` and
    /// `install_list_native_item_provider(provider)` — and exposes
    /// the captured provider so tests can pretend to be the C++
    /// list and exercise the callbacks.
    #[derive(Default)]
    struct CapturingRenderer {
        next_id: u32,
        last_count: Rc<RefCell<Option<i32>>>,
        installed: Rc<RefCell<Option<NativeItemProvider>>>,
    }

    impl DynRenderer for CapturingRenderer {
        fn create_element(&mut self, _tag: ElementTag) -> Element {
            let id = self.next_id;
            self.next_id += 1;
            Element::from_raw(id)
        }
        fn create_element_by_name(&mut self, _tag: &str) -> Element {
            let id = self.next_id;
            self.next_id += 1;
            Element::from_raw(id)
        }
        fn release_element(&mut self, _h: Element) {}
        fn set_attribute(&mut self, _h: Element, _k: &str, _v: &str) {}
        fn set_inline_styles(&mut self, _h: Element, _css: &str) {}
        fn set_update_list_info(&mut self, _h: Element, count: i32) {
            *self.last_count.borrow_mut() = Some(count);
        }
        fn install_list_native_item_provider(
            &mut self,
            _h: Element,
            provider: NativeItemProvider,
        ) -> bool {
            *self.installed.borrow_mut() = Some(provider);
            true
        }
        fn append_child(&mut self, _p: Element, _c: Element) {}
        fn remove_child(&mut self, _p: Element, _c: Element) {}
        fn set_event_listener(
            &mut self,
            _h: Element,
            _n: &str,
            _bt: BindType,
            _cb: Box<dyn Fn(crate::value::WhiskerValue) + 'static>,
        ) {
        }
        fn set_root(&mut self, _p: Element) {}
        fn flush(&mut self) {}
    }

    fn with_capturing<R>(
        f: impl FnOnce(Rc<RefCell<Option<i32>>>, Rc<RefCell<Option<NativeItemProvider>>>) -> R,
    ) -> R {
        crate::reactive::__reset_for_tests();
        let recorder = CapturingRenderer::default();
        let last_count = recorder.last_count.clone();
        let installed = recorder.installed.clone();
        let prev = install_renderer(Box::new(recorder));
        let r = f(last_count, installed);
        uninstall_renderer(prev);
        r
    }

    #[test]
    fn list_mount_broadcasts_initial_count_and_installs_provider() {
        with_capturing(|count, installed| {
            let owner = create_owner(None);
            with_owner(owner, || {
                let handle = create_element_by_name("list");
                list_mount(
                    handle,
                    || vec![10_i32, 20, 30],
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();

            assert_eq!(*count.borrow(), Some(3));
            assert!(installed.borrow().is_some());
            dispose_owner(owner);
        });
    }

    #[test]
    fn provider_component_at_index_returns_distinct_signs_and_runs_body() {
        with_capturing(|_count, installed| {
            let owner = create_owner(None);
            let body_calls = Rc::new(RefCell::new(Vec::<i32>::new()));
            let bc = body_calls.clone();
            with_owner(owner, || {
                let handle = create_element_by_name("list");
                list_mount(
                    handle,
                    || vec![100_i32, 200, 300],
                    |x| *x,
                    move |x| {
                        bc.borrow_mut().push(x);
                        create_element_by_name("list-item")
                    },
                );
            });
            flush();

            // Pretend to be the C++ list and fetch items 0, 1, 2.
            let mut provider = installed.borrow_mut().take().expect("provider installed");
            let s0 = (provider.component_at_index)(0, 0, false);
            let s1 = (provider.component_at_index)(1, 0, false);
            let s2 = (provider.component_at_index)(2, 0, false);

            assert_ne!(s0, INVALID_ITEM_INDEX);
            assert_ne!(s1, INVALID_ITEM_INDEX);
            assert_ne!(s2, INVALID_ITEM_INDEX);
            assert_ne!(s0, s1);
            assert_ne!(s1, s2);
            assert_eq!(*body_calls.borrow(), vec![100, 200, 300]);

            dispose_owner(owner);
        });
    }

    #[test]
    fn provider_out_of_range_index_returns_invalid() {
        with_capturing(|_count, installed| {
            let owner = create_owner(None);
            with_owner(owner, || {
                let handle = create_element_by_name("list");
                list_mount(
                    handle,
                    || vec![1_i32, 2],
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider installed");
            assert_eq!(
                (provider.component_at_index)(5, 0, false),
                INVALID_ITEM_INDEX
            );
            dispose_owner(owner);
        });
    }

    #[test]
    fn enqueue_component_disposes_the_slot_owner() {
        with_capturing(|_count, installed| {
            let owner = create_owner(None);
            let drop_observed = Rc::new(RefCell::new(false));

            with_owner(owner, || {
                let handle = create_element_by_name("list");
                let dr = drop_observed.clone();
                list_mount(
                    handle,
                    || vec![1_i32],
                    |x| *x,
                    move |_x| {
                        // Register an on_cleanup hook so we can
                        // observe when this slot's owner is disposed.
                        let dr = dr.clone();
                        crate::reactive::on_cleanup(move || {
                            *dr.borrow_mut() = true;
                        });
                        create_element_by_name("list-item")
                    },
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider installed");
            let sign = (provider.component_at_index)(0, 0, false);
            assert_ne!(sign, INVALID_ITEM_INDEX);
            assert!(!*drop_observed.borrow(), "before enqueue");

            let enqueue = provider
                .enqueue_component
                .as_mut()
                .expect("enqueue installed");
            (enqueue)(sign);
            assert!(
                *drop_observed.borrow(),
                "after enqueue, slot owner disposed"
            );

            dispose_owner(owner);
        });
    }

    #[test]
    fn changing_items_rebroadcasts_count() {
        with_capturing(|count, _installed| {
            let owner = create_owner(None);
            let (items_read, items_write) = crate::reactive::signal(vec![1_i32, 2, 3]);
            with_owner(owner, || {
                let handle = create_element_by_name("list");
                list_mount(
                    handle,
                    move || items_read.get(),
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();
            assert_eq!(*count.borrow(), Some(3));

            items_write.set(vec![1, 2, 3, 4, 5]);
            flush();
            assert_eq!(*count.borrow(), Some(5));

            items_write.set(vec![]);
            flush();
            assert_eq!(*count.borrow(), Some(0));

            dispose_owner(owner);
        });
    }

    #[test]
    fn on_cleanup_disposes_remaining_slot_owners() {
        with_capturing(|_count, installed| {
            let outer = create_owner(None);
            let drops = Rc::new(RefCell::new(0_u32));

            with_owner(outer, || {
                let handle = create_element_by_name("list");
                let drops = drops.clone();
                list_mount(
                    handle,
                    || vec![1_i32, 2, 3],
                    |x| *x,
                    move |_x| {
                        let d = drops.clone();
                        crate::reactive::on_cleanup(move || *d.borrow_mut() += 1);
                        create_element_by_name("list-item")
                    },
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider installed");
            let _ = (provider.component_at_index)(0, 0, false);
            let _ = (provider.component_at_index)(1, 0, false);
            assert_eq!(*drops.borrow(), 0);

            // Outer-owner disposal should sweep the live slots.
            // (The provider is dropped here so its captured slot
            // map drops too — the on_cleanup hook is what reaches
            // into the slot map BEFORE that to dispose owners.)
            drop(provider);
            dispose_owner(outer);
            assert_eq!(
                *drops.borrow(),
                2,
                "both live slots disposed on owner sweep"
            );
        });
    }
}
