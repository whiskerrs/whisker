//! Control-flow components: [`show`] (if/else) and [`for_each`]
//! (keyed list).
//!
//! Both create a wrapper `view` element and an effect that watches
//! a reactive source (`when` for `show`, `each` for `for_each`).
//! When the source changes, the effect rebuilds the wrapper's
//! children — disposing item owners that disappear and creating
//! new ones for additions. Items that survive across re-renders
//! keep their owners (and therefore their reactive state) intact.
//!
//! The `render!` macro recognises `Show { ... }` / `For { ... }`
//! call sites and emits calls into this module; users can also
//! call these functions directly for non-macro programmatic use.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use crate::element::ElementTag;
use crate::reactive::{create_owner, dispose_owner, effect, with_owner};

use super::handle::Element;
use super::into_view::{IntoView, View};
use super::renderer::create_element;

/// Conditional rendering. When `when()` is true the `children`
/// closure's view is mounted into the returned wrapper element; when
/// false, the `fallback` closure (if any) is mounted instead.
///
/// On every flip the mounted side's owner is disposed (cascading
/// cleanups + freeing reactive nodes) before the other side is
/// instantiated, so state from the previous branch is not
/// accidentally retained.
///
/// Returns the wrapper [`Element`] — the parent attaches this
/// once and the inner content swaps in place.
pub fn show(
    when: impl Fn() -> bool + 'static,
    children: impl Fn() -> View + 'static,
    fallback: Option<Box<dyn Fn() -> View + 'static>>,
) -> Element {
    let wrapper = create_element(ElementTag::View);
    // Track the currently-mounted owner so we can dispose it on the
    // next flip. `Rc<RefCell>` because the effect closure runs many
    // times and needs interior mutability.
    let mounted: Rc<RefCell<Option<crate::reactive::OwnerId>>> = Rc::new(RefCell::new(None));

    effect(move || {
        // 1. Detach every current child of the wrapper before
        // disposing the previous branch owner — same rationale as
        // `suspense` below. `#[component]` bodies inside the branch
        // create detached owners that the dispose cascade doesn't
        // reach, so their elements would otherwise stay attached
        // through a `when` flip.
        for child in super::children_of(wrapper) {
            super::remove_child(wrapper, child);
        }

        // 2. Dispose whatever's currently mounted, if anything.
        let prev = mounted.borrow_mut().take();
        if let Some(o) = prev {
            dispose_owner(o);
        }

        // 3. Mount the appropriate branch under a fresh owner.
        let branch_owner = create_owner(None);
        let cond = when();
        with_owner(branch_owner, || {
            let view = if cond {
                children()
            } else if let Some(fb) = fallback.as_ref() {
                fb()
            } else {
                View::Empty
            };
            view.attach_to(wrapper);
        });
        *mounted.borrow_mut() = Some(branch_owner);
    });

    wrapper
}

/// Keyed list rendering. `each` is re-evaluated on every dep change
/// (it's the effect's tracked input); for each item in the returned
/// vector, the macro/runtime calls `key(&item)` to derive a stable
/// identity. Items whose keys match a previous render are reused
/// (their owners and per-item reactive state survive); items that
/// disappear have their owners disposed.
///
/// **Reordering**: when reused items appear in a different order,
/// we detach them all (in the previous order) and re-attach them
/// (in the new order) so the wrapper's child list reflects the new
/// position. This is the simplest correct reorder strategy; if list
/// churn ever becomes a perf concern, a smarter LIS-based moves
/// path can replace the detach-all step.
pub fn for_each<T, K, V, EachFn, KeyFn, ChildFn>(
    each: EachFn,
    key: KeyFn,
    children: ChildFn,
) -> Element
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    let wrapper = create_element(ElementTag::View);
    // Default to a vertical list. Lynx `view` defaults to
    // `flex-direction: row` (memory: lynx_view_flex_direction_default);
    // for lists that's almost always the wrong axis.
    super::set_inline_styles(wrapper, "display: flex; flex-direction: column;");
    // Per-key bookkeeping. We track the owner so we can dispose
    // removed items, plus the attached element handles so we can
    // re-attach in the new order during a reorder.
    struct Entry {
        owner: crate::reactive::OwnerId,
        handles: Vec<Element>,
    }
    let entries: Rc<RefCell<HashMap<K, Entry>>> = Rc::new(RefCell::new(HashMap::new()));

    effect(move || {
        let items = each();
        let mut new_entries: HashMap<K, Entry> = HashMap::new();
        let mut new_keys_in_order: Vec<K> = Vec::with_capacity(items.len());

        // Pull the old map out so we can move entries across
        // without holding the borrow during user-code calls
        // (`children()`).
        let mut old = std::mem::take(&mut *entries.borrow_mut());

        for item in items {
            let k = key(&item);
            if let Some(existing) = old.remove(&k) {
                new_entries.insert(k.clone(), existing);
            } else {
                let item_owner = create_owner(None);
                let handles = with_owner(item_owner, || {
                    let view = children(item).into_view();
                    view.attach_to(wrapper)
                });
                new_entries.insert(
                    k.clone(),
                    Entry {
                        owner: item_owner,
                        handles,
                    },
                );
            }
            new_keys_in_order.push(k);
        }

        // Anything still in `old` has been removed — dispose so
        // reactive nodes + cleanups fire. We detach the elements
        // first, then dispose the owner. Order matters: dispose
        // before detach would leave the now-freed-owner trying to
        // touch the renderer for no reason.
        for (_, entry) in old.drain() {
            for h in &entry.handles {
                super::remove_child(wrapper, *h);
            }
            dispose_owner(entry.owner);
        }

        // Re-attach all kept entries in the new order. We're
        // unconditional here (rather than diffing) — for v1 the
        // simpler implementation wins; the typical churn shape
        // (append, prepend, single-row insert) is still cheap.
        // First detach everything we kept...
        for k in &new_keys_in_order {
            if let Some(entry) = new_entries.get(k) {
                for h in &entry.handles {
                    super::remove_child(wrapper, *h);
                }
            }
        }
        // ...then re-attach in the desired order.
        for k in &new_keys_in_order {
            if let Some(entry) = new_entries.get(k) {
                for h in &entry.handles {
                    super::append_child(wrapper, *h);
                }
            }
        }

        *entries.borrow_mut() = new_entries;
    });

    wrapper
}
