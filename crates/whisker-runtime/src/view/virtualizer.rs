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
//!    build the element for `items[i]` *on demand* under a per-slot
//!    owner **parented to the setup owner** (so the item inherits the
//!    list's context — `use_context` / `use_navigator` work even though
//!    the callback fires outside any reactive scope), stamp a STABLE
//!    `item-key` (so reorders diff correctly), **append it to the list**
//!    (the provider contract — see below), and return its Lynx sign.
//!    When the slot scrolls out `enqueue_component(sign)` removes it from
//!    the list and disposes the owner (recycle / release). Only the
//!    visible + buffered slots are attached at once, so this stays
//!    virtualised even though items are real children while live.
//!
//! On owner disposal every live slot is disposed and the provider is
//! released through `whisker-driver`'s `trampoline_free`.
//!
//! # Provider contract (verified on device)
//!
//! Lynx `ListElement::ComponentAtIndex` requires the embedder callback
//! to **attach the item to the list** (`append_child(list, item)`) and
//! return its `impl_id`; it then runs `OnComponentFinished` → layout
//! over that freshly-appended subtree. Returning the sign *without*
//! appending leaves the item unparented and the native list crashes in
//! `OnListItemWillAppear` (null `element_container`) — found on device,
//! fixed by the `append_child` here. Re-entrant element creation during
//! the callback is safe (the bridge renderer holds no field borrow
//! across an FFI call, #214).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::reactive::{Owner, effect, on_cleanup};

use super::apply::apply_attr_owned;
use super::handle::Element;
use super::list_provider::{INVALID_ITEM_INDEX, NativeItemProvider};
use super::renderer::{
    append_child, element_sign, install_list_native_item_provider, remove_child, set_attribute_int,
    set_update_list_info, update_list_actions,
};

/// Send a data-source update as the MINIMAL action set against the
/// previous key list: the longest common prefix and suffix survive
/// untouched (keeping their native `ItemHolder` identity — which is
/// what lets the list hold its scroll position across an append), and
/// only the middle window is expressed as remove+insert.
///
/// Index contract (what Lynx's `AdapterHelper` expects, matching
/// ReactLynx's `ListUpdateInfoRecording` output): removals are
/// ascending indices into the PRE-update list and apply first; insert
/// positions are ascending splice points into the post-removal list.
///
/// A reorder that survives neither prefix nor suffix degrades to a
/// full-window remove+insert — same identity loss as the legacy full
/// replace, no worse. When the renderer can't deliver explicit actions
/// (a Lynx build predating the capi, or a test renderer), falls back
/// to the full-replace [`set_update_list_info`].
fn send_data_source_update(handle: Element, prev: &[String], keys: &[String]) {
    let (removals, inserts) = splice_diff(prev, keys);
    if removals.is_empty() && inserts.is_empty() {
        // Identical key list — nothing structural to update.
        return;
    }
    if !update_list_actions(handle, &removals, &inserts) {
        set_update_list_info(handle, keys, prev.len());
    }
}

/// The common-prefix/suffix splice between two key lists, as
/// `(removals, inserts)` in `AdapterHelper`'s index spaces (removals:
/// ascending pre-update indices; inserts: ascending `(position, key)`
/// splice points into the post-removal list).
fn splice_diff(prev: &[String], keys: &[String]) -> (Vec<i32>, Vec<(i32, String)>) {
    let p = prev
        .iter()
        .zip(keys.iter())
        .take_while(|(a, b)| a == b)
        .count();
    // The zip over the two post-prefix slices yields at most
    // `min(prev.len(), keys.len()) - p` pairs, so the suffix can never
    // overlap the prefix.
    let s = prev[p..]
        .iter()
        .rev()
        .zip(keys[p..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let removals: Vec<i32> = (p..prev.len() - s).map(|i| i as i32).collect();
    let inserts: Vec<(i32, String)> = keys[p..keys.len() - s]
        .iter()
        .enumerate()
        .map(|(k, key)| ((p + k) as i32, key.clone()))
        .collect();
    (removals, inserts)
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
    // Live slots: Lynx sign -> (element, owner). Each `component_at_index`
    // builds a FRESH element (the native list requires an element ready to
    // bind to *this* item_holder — reusing one that's live elsewhere breaks
    // its diff on data updates), tracks it here, and returns its sign.
    let live: Rc<RefCell<HashMap<i32, (Element, Owner)>>> = Rc::new(RefCell::new(HashMap::new()));
    // Enqueued slots awaiting disposal. The native `EnqueueElement` calls
    // `enqueue_component` and THEN detaches the element itself
    // (`RemoveListItemPaintingNode` + `DetachChild`), so disposing during the
    // callback is a use-after-free. We defer: an enqueued slot lands here and
    // is disposed at the start of the next provider call / data update, by
    // which point the native has finished detaching it.
    let pending: Rc<RefCell<Vec<(Element, Owner)>>> = Rc::new(RefCell::new(Vec::new()));
    // Stable `item-key` ids: a logical key keeps the same
    // `item-key="w_{id}"` across rebuilds so the native diff can move it.
    let key_ids: Rc<RefCell<HashMap<K, u64>>> = Rc::new(RefCell::new(HashMap::new()));
    let next_id: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let layout_id: Rc<Cell<i64>> = Rc::new(Cell::new(0));
    // Item-key list from the previous data-source update — the minimal
    // diff (common prefix/suffix splice) is computed against it, so
    // untouched items keep their native identity and the list can hold
    // its scroll position across appends.
    let prev_keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let key = Rc::new(key);
    let build = Rc::new(build);
    // Anchor slot owners to the owner active at SETUP (the list component's
    // scope) so items built on demand in the native callback still inherit
    // context — `use_context` / `use_navigator` walk the owner chain.
    // `component_at_index` fires outside any reactive scope, so a detached
    // slot owner would sever the chain (on-device: `use_navigator() outside
    // a Router`).
    let parent_owner = Owner::new(None);

    // Dispose the deferred batch. Safe to call whenever control is back in a
    // whisker frame (the native has finished its `EnqueueElement` detach).
    let flush_pending = {
        let pending = pending.clone();
        move || {
            let batch: Vec<(Element, Owner)> = pending.borrow_mut().drain(..).collect();
            for (element, owner) in batch {
                remove_child(handle, element);
                owner.dispose();
            }
        }
    };

    // Effect: items() -> snapshot + stable item-key list + layout-id bump +
    // data-source update (drives the native diff).
    {
        let current_items = current_items.clone();
        let layout_id = layout_id.clone();
        let prev_keys = prev_keys.clone();
        let key_ids = key_ids.clone();
        let next_id = next_id.clone();
        let key = key.clone();
        let flush_pending = flush_pending.clone();
        effect(move || {
            flush_pending();
            let new_items = items();
            // Stable item-key for every item in current order (assign an id
            // to first-seen keys). `component_at_index` stamps the matching
            // `item-key="w_{id}"` on each built element, so the native list
            // reconciles them and can diff a reorder.
            let keys: Vec<String> = {
                let mut ids = key_ids.borrow_mut();
                new_items
                    .iter()
                    .map(|item| {
                        let k = key(item);
                        let id = match ids.get(&k) {
                            Some(id) => *id,
                            None => {
                                let id = next_id.get();
                                next_id.set(id + 1);
                                ids.insert(k, id);
                                id
                            }
                        };
                        format!("w_{id}")
                    })
                    .collect()
            };
            *current_items.borrow_mut() = new_items;
            let lid = layout_id.get();
            layout_id.set(lid + 1);
            set_attribute_int(handle, "layout-id", lid);
            send_data_source_update(handle, &prev_keys.borrow(), &keys);
            *prev_keys.borrow_mut() = keys;
        });
    }

    let provider = NativeItemProvider {
        component_at_index: {
            let current_items = current_items.clone();
            let live = live.clone();
            let key_ids = key_ids.clone();
            let next_id = next_id.clone();
            let key = key.clone();
            let build = build.clone();
            let flush_pending = flush_pending.clone();
            Box::new(move |index, _op_id, _reuse| {
                // Dispose the previous batch of enqueued-and-detached elements.
                flush_pending();
                let item = match current_items.borrow().get(index as usize) {
                    Some(t) => t.clone(),
                    None => return INVALID_ITEM_INDEX,
                };
                let k = key(&item);
                // Stable id for the `item-key` (assign on first sight).
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
                // Build a FRESH element under a per-item owner (context
                // inherited via `parent_owner`), stamp the stable item-key,
                // and ATTACH it — Lynx `ComponentAtIndex` then runs
                // `OnComponentFinished` → layout over the appended subtree.
                // Returning a sign without appending crashes the native list.
                let owner = Owner::new(Some(parent_owner));
                let element = owner.with(|| (build)(item));
                apply_attr_owned::<_, String>(element, "item-key".to_string(), format!("w_{id}"));
                append_child(handle, element);
                let sign = element_sign(element);
                live.borrow_mut().insert(sign, (element, owner));
                sign
            })
        },
        enqueue_component: Some({
            let live = live.clone();
            let pending = pending.clone();
            Box::new(move |sign| {
                // Move the slot to the deferred-disposal queue. Do NOT destroy
                // it here — the native still detaches the element after this
                // callback returns (use-after-free otherwise).
                if let Some(slot) = live.borrow_mut().remove(&sign) {
                    pending.borrow_mut().push(slot);
                }
            })
        }),
    };
    install_list_native_item_provider(handle, provider);

    on_cleanup(move || {
        flush_pending();
        // `parent_owner` cascades to every live slot owner (each is its
        // child), freeing their elements.
        parent_owner.dispose();
        live.borrow_mut().clear();
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
        fn set_update_list_info(&self, _h: Element, item_keys: &[String], _prev_count: usize) {
            *self.last_count.borrow_mut() = Some(item_keys.len() as i32);
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
    fn enqueue_defers_disposal_until_next_provider_call() {
        with_capturing(|_count, installed, _| {
            let owner = Owner::new(None);
            let dropped = Rc::new(RefCell::new(0u32));
            owner.with(|| {
                let handle = create_element_by_name("list");
                let dr = dropped.clone();
                virtualize(
                    handle,
                    || vec![1_i32, 2],
                    |x| *x,
                    move |_x| {
                        let dr = dr.clone();
                        crate::reactive::on_cleanup(move || *dr.borrow_mut() += 1);
                        create_element_by_name("list-item")
                    },
                );
            });
            flush();

            let mut provider = installed.borrow_mut().take().expect("provider");
            let s0 = (provider.component_at_index)(0, 0, false);
            assert_ne!(s0, INVALID_ITEM_INDEX);

            // Enqueue must NOT dispose synchronously (native still detaches the
            // element after this returns → use-after-free otherwise).
            let enqueue = provider.enqueue_component.as_mut().expect("enqueue");
            (enqueue)(s0);
            assert_eq!(*dropped.borrow(), 0, "enqueue defers disposal");

            // The next provider call flushes the deferred batch.
            let _s1 = (provider.component_at_index)(1, 1, false);
            assert_eq!(*dropped.borrow(), 1, "deferred slot disposed on next call");

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

    fn ks(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn splice_diff_append_is_insert_only() {
        let (removals, inserts) = splice_diff(&ks(&["a", "b"]), &ks(&["a", "b", "c", "d"]));
        assert!(removals.is_empty());
        assert_eq!(inserts, vec![(2, "c".to_string()), (3, "d".to_string())]);
    }

    #[test]
    fn splice_diff_prepend_is_insert_at_zero() {
        let (removals, inserts) = splice_diff(&ks(&["a", "b"]), &ks(&["x", "a", "b"]));
        assert!(removals.is_empty());
        assert_eq!(inserts, vec![(0, "x".to_string())]);
    }

    #[test]
    fn splice_diff_mid_replace_touches_only_the_window() {
        // [a b c d e] -> [a b X Y e]: remove old 2,3; insert X@2, Y@3.
        let (removals, inserts) = splice_diff(
            &ks(&["a", "b", "c", "d", "e"]),
            &ks(&["a", "b", "X", "Y", "e"]),
        );
        assert_eq!(removals, vec![2, 3]);
        assert_eq!(inserts, vec![(2, "X".to_string()), (3, "Y".to_string())]);
    }

    #[test]
    fn splice_diff_removal_only() {
        let (removals, inserts) = splice_diff(&ks(&["a", "b", "c"]), &ks(&["a", "c"]));
        assert_eq!(removals, vec![1]);
        assert!(inserts.is_empty());
    }

    #[test]
    fn splice_diff_identical_is_empty() {
        let (removals, inserts) = splice_diff(&ks(&["a", "b"]), &ks(&["a", "b"]));
        assert!(removals.is_empty());
        assert!(inserts.is_empty());
    }

    #[test]
    fn splice_diff_reorder_degrades_to_full_window() {
        // No common prefix/suffix survives a rotate — remove all, insert all.
        let (removals, inserts) = splice_diff(&ks(&["a", "b", "c"]), &ks(&["b", "c", "a"]));
        assert_eq!(removals, vec![0, 1, 2]);
        assert_eq!(
            inserts,
            vec![
                (0, "b".to_string()),
                (1, "c".to_string()),
                (2, "a".to_string())
            ]
        );
    }

    /// One recorded `update_list_actions` call: `(removals, inserts)`.
    type RecordedActions = (Vec<i32>, Vec<(i32, String)>);

    /// Renderer that accepts explicit actions (like the real bridge on a
    /// new-enough Lynx) — the virtualizer must prefer them and NOT fall
    /// back to the full replace.
    #[derive(Default)]
    struct ActionsRenderer {
        next_id: std::cell::Cell<u32>,
        actions: Rc<RefCell<Vec<RecordedActions>>>,
        full_replace_calls: Rc<RefCell<u32>>,
    }

    impl DynRenderer for ActionsRenderer {
        fn create_element(&self, _tag: ElementTag) -> Element {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            Element::from_raw(id)
        }
        fn create_element_by_name(&self, _tag: &str) -> Element {
            self.create_element(ElementTag::View)
        }
        fn release_element(&self, _h: Element) {}
        fn set_attribute(&self, _h: Element, _k: &str, _v: &str) {}
        fn set_inline_styles(&self, _h: Element, _css: &str) {}
        fn set_update_list_info(&self, _h: Element, _keys: &[String], _prev: usize) {
            *self.full_replace_calls.borrow_mut() += 1;
        }
        fn update_list_actions(
            &self,
            _h: Element,
            removals: &[i32],
            inserts: &[(i32, String)],
        ) -> bool {
            self.actions
                .borrow_mut()
                .push((removals.to_vec(), inserts.to_vec()));
            true
        }
        fn install_list_native_item_provider(&self, _h: Element, _p: NativeItemProvider) -> bool {
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

    #[test]
    fn append_sends_insert_only_actions_and_skips_full_replace() {
        crate::reactive::__reset_for_tests();
        let recorder = ActionsRenderer::default();
        let actions = recorder.actions.clone();
        let full_replace = recorder.full_replace_calls.clone();
        let prev = install_renderer(Box::new(recorder));

        let owner = Owner::new(None);
        let (items, set_items) = crate::reactive::signal(vec![1_i32, 2]).split();
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
        // Initial population: pure insert of both keys.
        assert_eq!(actions.borrow().len(), 1);
        assert_eq!(actions.borrow()[0].0, Vec::<i32>::new());
        assert_eq!(actions.borrow()[0].1.len(), 2);

        // Append: insert-only at the tail, untouched items unmentioned.
        set_items.set(vec![1_i32, 2, 3]);
        flush();
        assert_eq!(actions.borrow().len(), 2);
        let (removals, inserts) = actions.borrow()[1].clone();
        assert!(removals.is_empty());
        assert_eq!(inserts, vec![(2, "w_2".to_string())]);

        // Identical re-run: nothing sent.
        set_items.set(vec![1_i32, 2, 3]);
        flush();
        assert_eq!(actions.borrow().len(), 2);

        assert_eq!(*full_replace.borrow(), 0, "must not fall back");
        owner.dispose();
        uninstall_renderer(prev);
    }
}
