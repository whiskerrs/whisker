//! `virtualize` — the container-agnostic core that drives a native
//! item provider (Lynx's `componentAtIndex` / `enqueueComponent`
//! contract) on demand.
//!
//! Per whisker#83 the reusable core is split from the element it drives:
//! `ListMount = Virtualizer + <list> element`; a future `PagerMount`
//! would be `Virtualizer + <module element>`. This module is that
//! `Virtualizer` — it knows nothing about `<list>` specifically, only
//! that `handle` carries a native item provider and a count broadcast.
//!
//! # Model (pull, on-demand)
//!
//! 1. An effect subscribes to `items()`: on every change it snapshots
//!    the data, bumps a `layout-id` (so the native container registers
//!    a new data version), and broadcasts the new item count via
//!    `set_update_list_info`. **No elements are created here.**
//! 2. A [`NativeItemProvider`] is installed. When a slot enters the
//!    viewport the native container calls `component_at_index(i)`; we
//!    build the element for `items[i]` *on demand* under its own
//!    reactive owner, stamp a STABLE `item-key` (so reorders diff
//!    correctly), cache it by Lynx sign, and return the sign. When the
//!    slot scrolls out `enqueue_component(sign)` disposes the owner
//!    (recycle / release).
//!
//! On owner disposal every live slot is disposed and the provider is
//! released through `whisker-driver`'s `trampoline_free`.
//!
//! # ⚠️ On-device verification pending
//!
//! The pull contract here (build in `component_at_index`, no eager
//! tree-append; the native container attaches via the returned sign)
//! is the intended decoupled-list path but has NOT been verified on a
//! real device against the Lynx fork's `ListElement`. The eager path it
//! replaces is known-good. See `docs/list-design.md` § On-device
//! verifications. Re-entrant element creation itself IS safe (the
//! bridge renderer holds no field borrow across an FFI call, #214).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::reactive::{Owner, effect, on_cleanup};

use super::apply::apply_attr_owned;
use super::handle::Element;
use super::list_provider::{INVALID_ITEM_INDEX, NativeItemProvider};
use super::renderer::{
    element_sign, install_list_native_item_provider, set_attribute_int, set_update_list_info,
};

/// One live slot — its element and the reactive owner the builder ran
/// under. Dropped when the slot is enqueued out of the viewport, or
/// all-at-once on `virtualize`'s `on_cleanup`.
struct Slot {
    #[allow(dead_code)] // kept for future "manual move / measure" paths
    element: Element,
    owner: Owner,
}

/// Drive `handle`'s native item provider on demand.
///
/// - `handle` — the container element (already created) that carries a
///   native item provider (e.g. a `<list>`).
/// - `items` — reactive data source; re-read on any change it tracks.
/// - `key` — stable identity extractor. A logical key keeps the same
///   `item-key` across data updates so the native diff can move/remove.
/// - `build` — slot builder; called *once per slot creation* with a
///   clone of `items()[i]`, returns the slot element (e.g. a
///   `<list-item>`).
pub fn virtualize<T, K, ItemsFn, KeyFn, BuildFn>(
    handle: Element,
    items: ItemsFn,
    key: KeyFn,
    build: BuildFn,
) where
    T: Clone + 'static,
    K: Eq + Hash + Clone + 'static,
    ItemsFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    BuildFn: Fn(T) -> Element + 'static,
{
    let current_items: Rc<RefCell<Vec<T>>> = Rc::new(RefCell::new(Vec::new()));
    let slots: Rc<RefCell<HashMap<i32, Slot>>> = Rc::new(RefCell::new(HashMap::new()));
    // Stable `item-key` ids: a logical key is assigned an id on first
    // sight and keeps it, so its `item-key="w_{id}"` is stable across
    // reorders. Assigned lazily in `component_at_index` (only for the
    // items actually built), so it stays virtualisation-friendly.
    let key_ids: Rc<RefCell<HashMap<K, u64>>> = Rc::new(RefCell::new(HashMap::new()));
    let next_id: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let layout_id: Rc<Cell<i64>> = Rc::new(Cell::new(0));
    let key = Rc::new(key);
    let build = Rc::new(build);

    // Effect: items() -> snapshot + layout-id bump + count broadcast.
    {
        let current_items = current_items.clone();
        let layout_id = layout_id.clone();
        effect(move || {
            let new_items = items();
            let count = new_items.len() as i32;
            *current_items.borrow_mut() = new_items;
            let lid = layout_id.get();
            layout_id.set(lid + 1);
            set_attribute_int(handle, "layout-id", lid);
            set_update_list_info(handle, count);
        });
    }

    let provider = NativeItemProvider {
        component_at_index: {
            let current_items = current_items.clone();
            let slots = slots.clone();
            let key_ids = key_ids.clone();
            let next_id = next_id.clone();
            let key = key.clone();
            let build = build.clone();
            Box::new(move |index, _op_id, _reuse| {
                let item = match current_items.borrow().get(index as usize) {
                    Some(t) => t.clone(),
                    None => return INVALID_ITEM_INDEX,
                };
                // Stable id for this logical key (assign on first sight).
                let k = key(&item);
                let id = {
                    let mut ids = key_ids.borrow_mut();
                    match ids.get(&k) {
                        Some(id) => *id,
                        None => {
                            let id = next_id.get();
                            next_id.set(id + 1);
                            ids.insert(k, id);
                            id
                        }
                    }
                };
                // Per-slot owner: reactive subscriptions inside the
                // builder are cleaned up when the slot is enqueued out.
                let owner = Owner::new(None);
                let element = owner.with(|| (build)(item));
                apply_attr_owned::<_, String>(element, "item-key".to_string(), format!("w_{id}"));
                // Lynx keys slots by the element's *sign* (its bridge
                // `impl_id`), not by `Element::id()`. `enqueue_component`
                // is later called with the real sign.
                let sign = element_sign(element);
                slots.borrow_mut().insert(sign, Slot { element, owner });
                sign
            })
        },
        enqueue_component: Some({
            let slots = slots.clone();
            Box::new(move |sign| {
                if let Some(slot) = slots.borrow_mut().remove(&sign) {
                    slot.owner.dispose();
                }
            })
        }),
    };
    install_list_native_item_provider(handle, provider);

    // The provider's closures are released by the bridge's
    // `trampoline_free` when the container dies, which doesn't reach the
    // per-slot owners — dispose those explicitly here.
    on_cleanup(move || {
        let mut slots = slots.borrow_mut();
        for (_, slot) in slots.drain() {
            slot.owner.dispose();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::ElementTag;
    use crate::reactive::flush;
    use crate::view::renderer::{DynRenderer, install_renderer, uninstall_renderer};
    use crate::view::{BindType, create_element_by_name};

    /// Recording renderer: captures `set_update_list_info(count)`, the
    /// installed provider, and `item-key` stamps so tests can drive the
    /// provider like the C++ list and assert on identity.
    #[derive(Default)]
    struct CapturingRenderer {
        next_id: std::cell::Cell<u32>,
        last_count: Rc<RefCell<Option<i32>>>,
        installed: Rc<RefCell<Option<NativeItemProvider>>>,
        item_keys: Rc<RefCell<HashMap<u32, String>>>,
    }

    impl CapturingRenderer {
        fn alloc_id(&self) -> u32 {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            id
        }
    }

    impl DynRenderer for CapturingRenderer {
        fn create_element(&self, _tag: ElementTag) -> Element {
            Element::from_raw(self.alloc_id())
        }
        fn create_element_by_name(&self, _tag: &str) -> Element {
            Element::from_raw(self.alloc_id())
        }
        fn release_element(&self, _h: Element) {}
        fn element_sign(&self, handle: Element) -> i32 {
            handle.id() as i32
        }
        fn set_attribute(&self, h: Element, k: &str, v: &str) {
            if k == "item-key" {
                self.item_keys.borrow_mut().insert(h.id(), v.to_string());
            }
        }
        fn set_inline_styles(&self, _h: Element, _css: &str) {}
        fn set_update_list_info(&self, _h: Element, count: i32) {
            *self.last_count.borrow_mut() = Some(count);
        }
        fn install_list_native_item_provider(
            &self,
            _h: Element,
            provider: NativeItemProvider,
        ) -> bool {
            *self.installed.borrow_mut() = Some(provider);
            true
        }
        fn append_child(&self, _p: Element, _c: Element) {}
        fn remove_child(&self, _p: Element, _c: Element) {}
        fn set_event_listener(
            &self,
            _h: Element,
            _n: &str,
            _bt: BindType,
            _cb: Box<dyn Fn(crate::value::WhiskerValue) + 'static>,
        ) {
        }
        fn set_root(&self, _p: Element) {}
        fn flush(&self) {}
    }

    #[allow(clippy::type_complexity)]
    fn with_capturing<R>(
        f: impl FnOnce(
            Rc<RefCell<Option<i32>>>,
            Rc<RefCell<Option<NativeItemProvider>>>,
            Rc<RefCell<HashMap<u32, String>>>,
        ) -> R,
    ) -> R {
        crate::reactive::__reset_for_tests();
        let recorder = CapturingRenderer::default();
        let last_count = recorder.last_count.clone();
        let installed = recorder.installed.clone();
        let item_keys = recorder.item_keys.clone();
        let prev = install_renderer(Box::new(recorder));
        let r = f(last_count, installed, item_keys);
        uninstall_renderer(prev);
        r
    }

    #[test]
    fn broadcasts_count_and_installs_provider() {
        with_capturing(|count, installed, _| {
            let owner = Owner::new(None);
            owner.with(|| {
                let handle = create_element_by_name("list");
                virtualize(
                    handle,
                    || vec![10_i32, 20, 30],
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();
            assert_eq!(*count.borrow(), Some(3));
            assert!(installed.borrow().is_some());
            owner.dispose();
        });
    }

    #[test]
    fn component_at_index_builds_and_stamps_stable_item_key() {
        with_capturing(|_count, installed, item_keys| {
            let owner = Owner::new(None);
            owner.with(|| {
                let handle = create_element_by_name("list");
                virtualize(
                    handle,
                    || vec![100_i32, 200, 300],
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider");
            let s0 = (provider.component_at_index)(0, 0, false);
            let s1 = (provider.component_at_index)(1, 0, false);
            assert_ne!(s0, INVALID_ITEM_INDEX);
            assert_ne!(s1, INVALID_ITEM_INDEX);
            assert_ne!(s0, s1);
            // item-keys stamped, distinct, positionally "w_0"/"w_1" here
            // because keys are first-seen in order.
            let keys = item_keys.borrow();
            assert_eq!(keys.get(&(s0 as u32)), Some(&"w_0".to_string()));
            assert_eq!(keys.get(&(s1 as u32)), Some(&"w_1".to_string()));
            owner.dispose();
        });
    }

    #[test]
    fn same_key_keeps_stable_item_key_across_reorder() {
        with_capturing(|_count, installed, item_keys| {
            let owner = Owner::new(None);
            let (items, set_items) = crate::reactive::signal(vec![1_i32, 2, 3]).split();
            owner.with(|| {
                let handle = create_element_by_name("list");
                virtualize(
                    handle,
                    move || items.get(),
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider");
            // Build key 3 at index 2 → gets some stable id.
            let sign_three_first = (provider.component_at_index)(2, 0, false);
            let key_three = item_keys
                .borrow()
                .get(&(sign_three_first as u32))
                .cloned()
                .unwrap();

            // Reorder so key 3 is now at index 0.
            set_items.set(vec![3_i32, 1, 2]);
            flush();
            let sign_three_again = (provider.component_at_index)(0, 0, false);
            let key_three_again = item_keys
                .borrow()
                .get(&(sign_three_again as u32))
                .cloned()
                .unwrap();

            // Same logical key → same item-key, even at a new index.
            assert_eq!(key_three, key_three_again);
            owner.dispose();
        });
    }

    #[test]
    fn enqueue_disposes_slot_owner() {
        with_capturing(|_count, installed, _| {
            let owner = Owner::new(None);
            let dropped = Rc::new(RefCell::new(false));
            owner.with(|| {
                let handle = create_element_by_name("list");
                let dr = dropped.clone();
                virtualize(
                    handle,
                    || vec![1_i32],
                    |x| *x,
                    move |_x| {
                        let dr = dr.clone();
                        crate::reactive::on_cleanup(move || *dr.borrow_mut() = true);
                        create_element_by_name("list-item")
                    },
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider");
            let sign = (provider.component_at_index)(0, 0, false);
            assert!(!*dropped.borrow());
            let enqueue = provider.enqueue_component.as_mut().expect("enqueue");
            (enqueue)(sign);
            assert!(*dropped.borrow(), "enqueue disposes the slot owner");
            owner.dispose();
        });
    }

    #[test]
    fn out_of_range_index_returns_invalid() {
        with_capturing(|_count, installed, _| {
            let owner = Owner::new(None);
            owner.with(|| {
                let handle = create_element_by_name("list");
                virtualize(
                    handle,
                    || vec![1_i32, 2],
                    |x| *x,
                    |_x| create_element_by_name("list-item"),
                );
            });
            flush();
            let mut provider = installed.borrow_mut().take().expect("provider");
            assert_eq!(
                (provider.component_at_index)(5, 0, false),
                INVALID_ITEM_INDEX
            );
            owner.dispose();
        });
    }
}
