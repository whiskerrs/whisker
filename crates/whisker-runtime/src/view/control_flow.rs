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

use super::handle::ElementHandle;
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
/// Returns the wrapper [`ElementHandle`] — the parent attaches this
/// once and the inner content swaps in place.
pub fn show(
    when: impl Fn() -> bool + 'static,
    children: impl Fn() -> View + 'static,
    fallback: Option<Box<dyn Fn() -> View + 'static>>,
) -> ElementHandle {
    let wrapper = create_element(ElementTag::View);
    // Track the currently-mounted owner so we can dispose it on the
    // next flip. `Rc<RefCell>` because the effect closure runs many
    // times and needs interior mutability.
    let mounted: Rc<RefCell<Option<crate::reactive::OwnerId>>> =
        Rc::new(RefCell::new(None));

    effect(move || {
        // 1. Dispose whatever's currently mounted, if anything.
        let prev = mounted.borrow_mut().take();
        if let Some(o) = prev {
            dispose_owner(o);
        }

        // 2. Mount the appropriate branch under a fresh owner.
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
/// **v1 ordering limitation**: when reused items appear in a
/// different order, they stay in their original position under the
/// wrapper. Pure-append / pure-remove / mixed-add-remove cases work
/// correctly; reorders look stale. A subsequent commit will fix
/// this by re-attaching reused items into the new order.
pub fn for_each<T, K, V, EachFn, KeyFn, ChildFn>(
    each: EachFn,
    key: KeyFn,
    children: ChildFn,
) -> ElementHandle
where
    T: 'static,
    K: Eq + Hash + Clone + 'static,
    V: IntoView + 'static,
    EachFn: Fn() -> Vec<T> + 'static,
    KeyFn: Fn(&T) -> K + 'static,
    ChildFn: Fn(T) -> V + 'static,
{
    let wrapper = create_element(ElementTag::View);
    // Map from key → (item owner, view) — view is kept so we can
    // re-attach reused items if we add reordering later. v1 ignores
    // the View beyond mount time.
    let entries: Rc<RefCell<HashMap<K, crate::reactive::OwnerId>>> =
        Rc::new(RefCell::new(HashMap::new()));

    effect(move || {
        let items = each();
        let mut new_entries: HashMap<K, crate::reactive::OwnerId> = HashMap::new();

        // Pull the old map out so we can move owners across without
        // holding the borrow during user-code calls (children()).
        let mut old = std::mem::take(&mut *entries.borrow_mut());

        for item in items {
            let k = key(&item);
            if let Some(existing) = old.remove(&k) {
                // Reuse the existing owner (and its reactive state).
                new_entries.insert(k, existing);
            } else {
                let item_owner = create_owner(None);
                with_owner(item_owner, || {
                    let view = children(item).into_view();
                    view.attach_to(wrapper);
                });
                new_entries.insert(k, item_owner);
            }
        }

        // Anything still in `old` has been removed from the list —
        // dispose its owner so its children, reactive nodes, and
        // cleanup callbacks all fire.
        for (_, owner) in old.drain() {
            dispose_owner(owner);
        }

        *entries.borrow_mut() = new_entries;
    });

    wrapper
}
