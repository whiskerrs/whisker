//! Control-flow primitives — [`show`] (if/else) and [`for_each`]
//! (keyed list) — implemented as wrapper-less [`ControlFlow`] views
//! per [`docs/wrapper-less-control-flow-design.md`](https://github.com/whiskerrs/whisker/blob/main/docs/wrapper-less-control-flow-design.md).
//!
//! ## Two attach paths
//!
//! - **Generic parent** (`view`, `scroll_view`, …): the control flow
//!   creates a single *phantom* anchor element
//!   ([`super::create_phantom_element`]) and installs an effect that
//!   inserts items as the anchor's preceding siblings. Phantoms live
//!   in Whisker's mirror only — Lynx is never told about them, so
//!   the rendered tree has **zero extra elements** vs the user's
//!   markup. The anchor still serves as a stable positional marker
//!   for the mirror's [`child_index`](super::child_index) lookup,
//!   which is what makes reactive updates survive hot-reload-induced
//!   sibling churn.
//!
//! - **`<list>` parent**: the control flow writes `(handle, sign)`
//!   tuples directly into the list's shared
//!   [`ListItemsHandle`](super::ListItemsHandle) and broadcasts the
//!   item count via [`super::set_update_list_info`] on every update.
//!   The list's native-item provider closure (installed in
//!   `list.__h()`) reads from the same `Rc<RefCell<…>>`, so the next
//!   `componentAtIndex` lookup sees the new state. No anchor — the
//!   items Vec **is** the positional index.
//!
//! ## Owner cascade
//!
//! Each item gets a detached owner (`create_owner(None)`); the
//! control flow's effect explicitly disposes the per-item owner when
//! the item leaves the diff. This matches the pre-refactor behaviour
//! — when the surrounding component is disposed, its owner cascade
//! reaches this module's effect owner, the effect cleanup detaches
//! attached children, and the per-item owners are disposed
//! individually so their reactive state + element handles release.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::reactive::{create_owner, dispose_owner, effect, with_owner, OwnerId};

use super::handle::Element;
use super::into_view::{ControlFlow, IntoView, ListItemsHandle, View};
use super::renderer::{
    append_child, child_index, create_phantom_element, remove_child, set_attribute,
    set_update_list_info,
};
use super::{element_sign, insert_child_at};

// ---------------------------------------------------------------------------
// `For` (keyed list)
// ---------------------------------------------------------------------------

/// Keyed-list control flow. `for_each(...)` constructs one; the
/// surrounding container builder (`view`, `scroll_view`, `list`)
/// receives it via its `.child(impl IntoView)` and either dispatches
/// to [`ControlFlow::attach_generic`] (anchor pattern) or
/// [`ControlFlow::attach_to_list`] (items-Vec pattern).
pub struct For<T, K, V, EachFn, KeyFn, ChildFn>
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    each: EachFn,
    key: KeyFn,
    children: ChildFn,
    _t: PhantomData<(T, K, V)>,
}

/// Per-key bookkeeping for one item kept across an effect rerun.
/// `handles` is a `Vec` because one item's `children(item)` view may
/// itself be a fragment — multiple leaf elements all owned by the
/// same per-item owner. The native-list path collapses this to one
/// (`list_item`) handle per key in practice; the generic path
/// tolerates fragments transparently.
struct ItemEntry {
    owner: OwnerId,
    handles: Vec<Element>,
}

impl<T, K, V, EachFn, KeyFn, ChildFn> For<T, K, V, EachFn, KeyFn, ChildFn>
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
}

impl<T, K, V, EachFn, KeyFn, ChildFn> IntoView for For<T, K, V, EachFn, KeyFn, ChildFn>
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    fn into_view(self) -> View {
        View::ControlFlow(Box::new(self))
    }
}

impl<T, K, V, EachFn, KeyFn, ChildFn> ControlFlow for For<T, K, V, EachFn, KeyFn, ChildFn>
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    fn attach_generic(self: Box<Self>, parent: Element) {
        let For {
            each,
            key,
            children,
            ..
        } = *self;

        // Anchor marks the end of `For`'s range in `parent`'s mirror
        // child list. It survives across effect reruns; items get
        // inserted ahead of it. Phantom = mirror-only; no Lynx
        // element is allocated for the anchor.
        let anchor = create_phantom_element();
        append_child(parent, anchor);

        let entries: Rc<RefCell<HashMap<K, ItemEntry>>> = Rc::new(RefCell::new(HashMap::new()));

        effect(move || {
            let items = each();
            let mut new_entries: HashMap<K, ItemEntry> = HashMap::new();
            let mut new_keys_in_order: Vec<K> = Vec::with_capacity(items.len());

            // Pull the old map out so we can move entries across
            // without holding the borrow during user-code calls
            // (`children()` may itself touch reactive state).
            let mut old = std::mem::take(&mut *entries.borrow_mut());

            for item in items {
                let k = key(&item);
                if let Some(existing) = old.remove(&k) {
                    new_entries.insert(k.clone(), existing);
                } else {
                    let item_owner = create_owner(None);
                    let handles = with_owner(item_owner, || {
                        // Materialise into `parent`; we'll detach +
                        // reinsert all handles in the new order
                        // immediately below, so position here is
                        // transient.
                        children(item).into_view().attach_to(parent)
                    });
                    new_entries.insert(
                        k.clone(),
                        ItemEntry {
                            owner: item_owner,
                            handles,
                        },
                    );
                }
                new_keys_in_order.push(k);
            }

            // Anything still in `old` was removed in this update —
            // detach + dispose owner. Detach before dispose so the
            // owner cascade doesn't try to release elements still
            // attached to the parent (mirror cleanup ordering).
            for (_, entry) in old.drain() {
                for h in &entry.handles {
                    remove_child(parent, *h);
                }
                dispose_owner(entry.owner);
            }

            // Detach every kept handle (some may have just been
            // inserted at the wrong position above), then re-insert
            // in the new order ahead of the anchor. The anchor's
            // mirror index is what tells us "where my range ends" —
            // we recompute it after each insertion because each
            // `insert_child_at` shifts the anchor right by 1.
            for k in &new_keys_in_order {
                if let Some(entry) = new_entries.get(k) {
                    for h in &entry.handles {
                        remove_child(parent, *h);
                    }
                }
            }
            for k in &new_keys_in_order {
                if let Some(entry) = new_entries.get(k) {
                    for h in &entry.handles {
                        let anchor_idx = child_index(parent, anchor).unwrap_or(0);
                        insert_child_at(parent, *h, anchor_idx);
                    }
                }
            }

            *entries.borrow_mut() = new_entries;
        });
    }

    fn attach_to_list(self: Box<Self>, list_handle: Element, items_handle: ListItemsHandle) {
        let For {
            each,
            key,
            children,
            ..
        } = *self;

        // Per-key entries — same shape as `attach_generic`, except
        // each item must be a `<list-item>` element (or one whose
        // top-level handle Lynx will accept as a list slot).
        let entries: Rc<RefCell<HashMap<K, ItemEntry>>> = Rc::new(RefCell::new(HashMap::new()));
        // Stable key ordering across reruns lets the items Vec
        // reflect the latest source order without ever rebuilding
        // it from a HashMap iteration (which would be
        // non-deterministic).
        let order: Rc<RefCell<Vec<K>>> = Rc::new(RefCell::new(Vec::new()));

        effect(move || {
            let new_items = each();
            let mut new_entries: HashMap<K, ItemEntry> = HashMap::new();
            let mut new_keys: Vec<K> = Vec::with_capacity(new_items.len());

            let mut old = std::mem::take(&mut *entries.borrow_mut());

            for (idx, item) in new_items.into_iter().enumerate() {
                let k = key(&item);
                if let Some(existing) = old.remove(&k) {
                    new_entries.insert(k.clone(), existing);
                } else {
                    let item_owner = create_owner(None);
                    let handles = with_owner(item_owner, || {
                        children(item).into_view().attach_to(list_handle)
                    });
                    // Tag each item handle with the positional key
                    // Lynx's `update-list-info` map expects. The list
                    // builder's static path uses the same convention.
                    for h in &handles {
                        set_attribute(*h, "item-key", &format!("w_{}", idx));
                    }
                    new_entries.insert(
                        k.clone(),
                        ItemEntry {
                            owner: item_owner,
                            handles,
                        },
                    );
                }
                new_keys.push(k);
            }

            // Detach + dispose anything that disappeared from the
            // diff.
            for (_, entry) in old.drain() {
                for h in &entry.handles {
                    remove_child(list_handle, *h);
                }
                dispose_owner(entry.owner);
            }

            // Rebuild the items Vec in the new key order, capturing
            // a fresh Lynx sign for each leaf handle (`element_sign`
            // looks the impl_id up in the renderer's side map).
            let mut new_items_vec: Vec<(Element, i32)> = Vec::with_capacity(new_keys.len());
            for k in &new_keys {
                if let Some(entry) = new_entries.get(k) {
                    for h in &entry.handles {
                        let sign = element_sign(*h);
                        new_items_vec.push((*h, sign));
                    }
                }
            }

            let count = new_items_vec.len() as i32;
            *items_handle.borrow_mut() = new_items_vec;
            *order.borrow_mut() = new_keys;
            *entries.borrow_mut() = new_entries;

            // Tell Lynx how many slots to lay out. The native item
            // provider closure already reads from `items_handle` via
            // its own Rc clone, so no provider re-install is needed.
            set_update_list_info(list_handle, count);
        });
    }
}

/// Keyed list rendering. `each` is re-evaluated on every dep change
/// (it's the effect's tracked input); for each item the macro/runtime
/// calls `key(&item)` to derive a stable identity. Items whose keys
/// match a previous render are reused (their owners and per-item
/// reactive state survive); items that disappear have their owners
/// disposed.
///
/// Returns a [`For`] value that implements [`IntoView`] →
/// [`View::ControlFlow`]. Container builders dispatch to either
/// [`ControlFlow::attach_generic`] (a phantom anchor + reactive
/// siblings) or [`ControlFlow::attach_to_list`] (items go into the
/// `<list>`'s shared Vec).
pub fn for_each<T, K, V, EachFn, KeyFn, ChildFn>(
    each: EachFn,
    key: KeyFn,
    children: ChildFn,
) -> For<T, K, V, EachFn, KeyFn, ChildFn>
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    For {
        each,
        key,
        children,
        _t: PhantomData,
    }
}

// ---------------------------------------------------------------------------
// `Show` (conditional)
// ---------------------------------------------------------------------------

/// Conditional control flow. Built by [`show`]; the surrounding
/// builder dispatches to [`ControlFlow::attach_generic`] (phantom
/// anchor + branch as anchor's preceding sibling) or
/// [`ControlFlow::attach_to_list`] (0 or 1 item in the list's items
/// Vec, depending on `when`).
pub struct Show {
    when: Box<dyn Fn() -> bool + 'static>,
    children: Box<dyn Fn() -> View + 'static>,
    fallback: Option<Box<dyn Fn() -> View + 'static>>,
}

impl IntoView for Show {
    fn into_view(self) -> View {
        View::ControlFlow(Box::new(self))
    }
}

impl ControlFlow for Show {
    fn attach_generic(self: Box<Self>, parent: Element) {
        let Show {
            when,
            children,
            fallback,
        } = *self;

        let anchor = create_phantom_element();
        append_child(parent, anchor);

        // Track the currently-mounted branch (one or zero handles
        // depending on whether `children`/`fallback` materialised an
        // element) and the owner under which it was instantiated.
        let mounted_owner: Rc<RefCell<Option<OwnerId>>> = Rc::new(RefCell::new(None));
        let mounted_handles: Rc<RefCell<Vec<Element>>> = Rc::new(RefCell::new(Vec::new()));

        effect(move || {
            // 1. Detach the previous branch's elements + dispose its
            // owner. `#[component]` bodies inside the branch register
            // their own detached owners — explicit `remove_child` is
            // the safety net.
            let prev_handles = std::mem::take(&mut *mounted_handles.borrow_mut());
            for h in prev_handles {
                remove_child(parent, h);
            }
            if let Some(o) = mounted_owner.borrow_mut().take() {
                dispose_owner(o);
            }

            // 2. Mount the branch chosen by `when()` under a fresh
            // owner.
            let branch_owner = create_owner(None);
            let cond = when();
            let new_handles = with_owner(branch_owner, || {
                let view = if cond {
                    children()
                } else if let Some(fb) = fallback.as_ref() {
                    fb()
                } else {
                    View::Empty
                };
                view.attach_to(parent)
            });

            // 3. Reposition the freshly-attached handles ahead of
            // the anchor (they landed at the parent's tail).
            for h in &new_handles {
                remove_child(parent, *h);
                let anchor_idx = child_index(parent, anchor).unwrap_or(0);
                insert_child_at(parent, *h, anchor_idx);
            }

            *mounted_owner.borrow_mut() = Some(branch_owner);
            *mounted_handles.borrow_mut() = new_handles;
        });
    }

    fn attach_to_list(self: Box<Self>, list_handle: Element, items_handle: ListItemsHandle) {
        let Show {
            when,
            children,
            fallback,
        } = *self;

        let mounted_owner: Rc<RefCell<Option<OwnerId>>> = Rc::new(RefCell::new(None));
        let mounted_handles: Rc<RefCell<Vec<Element>>> = Rc::new(RefCell::new(Vec::new()));

        effect(move || {
            // Tear down the previously-mounted branch (if any).
            let prev_handles = std::mem::take(&mut *mounted_handles.borrow_mut());
            for h in prev_handles {
                remove_child(list_handle, h);
            }
            if let Some(o) = mounted_owner.borrow_mut().take() {
                dispose_owner(o);
            }

            let branch_owner = create_owner(None);
            let cond = when();
            let new_handles = with_owner(branch_owner, || {
                let view = if cond {
                    children()
                } else if let Some(fb) = fallback.as_ref() {
                    fb()
                } else {
                    View::Empty
                };
                view.attach_to(list_handle)
            });

            for (idx, h) in new_handles.iter().enumerate() {
                set_attribute(*h, "item-key", &format!("w_show_{}", idx));
            }

            let new_items_vec: Vec<(Element, i32)> = new_handles
                .iter()
                .map(|h| (*h, element_sign(*h)))
                .collect();
            let count = new_items_vec.len() as i32;
            *items_handle.borrow_mut() = new_items_vec;
            *mounted_owner.borrow_mut() = Some(branch_owner);
            *mounted_handles.borrow_mut() = new_handles;

            set_update_list_info(list_handle, count);
        });
    }
}

/// Conditional rendering. When `when()` is true the `children`
/// closure's view is mounted; when false, the `fallback` closure
/// (if any) is mounted instead.
///
/// On every flip the mounted side's owner is disposed (cascading
/// cleanups + freeing reactive nodes) before the other side is
/// instantiated, so state from the previous branch is not
/// accidentally retained.
///
/// Returns a [`Show`] value that implements [`IntoView`] →
/// [`View::ControlFlow`]; container builders dispatch via
/// [`ControlFlow::attach_generic`] or
/// [`ControlFlow::attach_to_list`].
pub fn show(
    when: impl Fn() -> bool + 'static,
    children: impl Fn() -> View + 'static,
    fallback: Option<Box<dyn Fn() -> View + 'static>>,
) -> Show {
    Show {
        when: Box::new(when),
        children: Box::new(children),
        fallback,
    }
}
