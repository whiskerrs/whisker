//! Context — parent-to-descendant value passing via the owner tree.
//!
//! `provide_context::<T>(value)` stores a value in the current owner's
//! per-type slot. `use_context::<T>()` walks up the owner chain until
//! it finds a slot for `T`, returning a clone. `with_context` is the
//! borrow-without-Clone variant.
//!
//! Context lookups are O(depth-of-owner-tree). For typical UI trees
//! (a few dozen levels) this is fine without indexing optimisation.

use std::any::{Any, TypeId};

use super::runtime::Owner;
use super::with_runtime;

/// Provide a context value in the current owner. Subsequent
/// `use_context::<T>` / `with_context::<T>` calls inside this owner or
/// any descendant find this value (unless a closer descendant shadows
/// it).
///
/// Re-providing the same `T` in the same owner replaces the previous
/// entry.
///
/// No-op (with debug-build warning) if there is no current owner.
pub fn provide_context<T: 'static>(value: T) {
    let registered = with_runtime(|rt| {
        let Some(owner_id) = rt.current_owner() else {
            return false;
        };
        let Some(owner) = rt.owners.get_mut(owner_id) else {
            return false;
        };
        owner
            .contexts
            .insert(TypeId::of::<T>(), std::rc::Rc::new(value));
        true
    });
    if !registered {
        super::warn_no_owner("provide_context");
    }
}

/// Look up the nearest provided context of type `T`, returning a clone.
/// Returns `None` if no ancestor owner provides one.
pub fn use_context<T: 'static + Clone>() -> Option<T> {
    with_context::<T, _>(|v| v.clone())
}

/// Look up the nearest provided context of type `T` and run `f` with a
/// borrow of it. Returns `None` if no ancestor owner provides one.
///
/// The borrow on the value is held only for the duration of `f`. The
/// runtime borrow is dropped before `f` is invoked, so `f` is free to
/// call back into the runtime (signals, effects, nested context
/// lookups all work).
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R> {
    // First locate the owner that holds the context. We can't return
    // a reference into the arena (borrow-checker, plus we want `f` to
    // re-enter the runtime), so we instead do a two-step: find +
    // dispatch. The downside is two borrows per lookup, but contexts
    // are not on a hot path.
    let owner_id = with_runtime(|rt| find_owner_with::<T>(rt, rt.current_owner()))?;

    // Clone the `Rc` handle out under a short borrow, then DROP the
    // runtime borrow before invoking `f`. This is what makes `f` free
    // to re-enter the runtime (read signals, create effects, nested
    // `use_context`) — calling `f` while the thread-local runtime was
    // still borrowed would double-borrow its `RefCell` and panic.
    //
    // Holding our own `Rc` clone also means the value stays alive for
    // the whole call even if `f` re-provides the same `T` on this owner
    // (which would replace the map entry); `f` simply observes the
    // value that was current at lookup time.
    let any_rc: std::rc::Rc<dyn Any> = with_runtime(|rt| {
        let owner = rt.owners.get(owner_id)?;
        owner.contexts.get(&TypeId::of::<T>()).cloned()
    })?;
    let typed: &T = any_rc
        .downcast_ref::<T>()
        .expect("context type tag mismatched stored value");
    Some(f(typed))
}

/// Walk from `start` upward through `parent` links, returning the
/// first owner that has a context of type `T`. Returns `None` if no
/// ancestor (including `start`) has one.
fn find_owner_with<T: 'static>(
    rt: &super::runtime::ReactiveRuntime,
    start: Option<Owner>,
) -> Option<Owner> {
    let type_id = TypeId::of::<T>();
    let mut cursor = start;
    while let Some(id) = cursor {
        let owner = rt.owners.get(id)?;
        if owner.contexts.contains_key(&type_id) {
            return Some(id);
        }
        cursor = owner.parent;
    }
    None
}
