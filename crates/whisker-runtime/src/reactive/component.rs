//! Component scoping, lifecycle, and hot-reload owner registry.
//!
//! Users normally interact with this module through the
//! `#[component]` proc-macro, which expands a function definition
//! into a body that:
//!
//! 1. Creates a fresh owner with [`mount_component`].
//! 2. Runs the user's body inside that owner.
//! 3. Returns the resulting view, leaving the owner alive (the parent
//!    keeps the handle; disposing the parent will cascade).
//!
//! The macro also passes its own fn pointer to
//! [`register_component`] so the Strategy C hot-reload path (A6) can
//! map subsecond-patched fn pointers back to live owners.
//!
//! Lifecycle hooks:
//!
//! - [`on_mount`] — registered against the current owner; fires once
//!   on the next [`flush_mounts`]. The renderer (A3) calls
//!   `flush_mounts` after appending the component's view to its
//!   parent.
//! - `on_cleanup` lives in `owner.rs` — symmetric LIFO callback that
//!   fires when the owner is disposed.

use super::owner::{create_owner, dispose_owner, with_owner};
use super::runtime::OwnerId;
use super::with_runtime;

/// Mount a component: create a fresh child owner, register `fn_ptr`
/// against it for hot reload, run `body` inside that owner, and
/// return both the owner id and the body's result.
///
/// The caller is responsible for keeping the returned `OwnerId` alive
/// (e.g. attaching it to the parent component's owner-children list
/// via the renderer) and for disposing it when the component
/// unmounts. The owner is already linked as a child of the
/// current-owner-at-call-time, so calling [`dispose_owner`] on an
/// ancestor will cascade.
pub fn mount_component<R>(fn_ptr: *const (), body: impl FnOnce() -> R) -> (OwnerId, R) {
    let owner = create_owner(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    let result = with_owner(owner, body);
    (owner, result)
}

/// Dispose a component owner *and* deregister it from
/// `component_owners`. Use this instead of plain `dispose_owner` for
/// owners created via `mount_component`.
pub fn unmount_component(owner: OwnerId) {
    let fn_ptr = with_runtime(|rt| rt.owners.get(owner).and_then(|o| o.mount_fn));
    if let Some(fp) = fn_ptr {
        with_runtime(|rt| {
            if let Some(list) = rt.component_owners.get_mut(&fp) {
                list.retain(|o| *o != owner);
                if list.is_empty() {
                    rt.component_owners.remove(&fp);
                }
            }
        });
    }
    dispose_owner(owner);
}

/// Register `f` as a post-mount callback for the current owner. Fires
/// once on the next [`flush_mounts`] call (driven by the renderer
/// after the component's view is appended to its parent).
///
/// No-op (with debug-build warning) if there is no current owner.
pub fn on_mount(f: impl FnOnce() + 'static) {
    let registered = with_runtime(|rt| {
        if rt.current_owner().is_none() {
            return false;
        }
        rt.pending_mounts.push(Box::new(f));
        true
    });
    if !registered {
        super::warn_no_owner("on_mount");
    }
}

/// Run all queued on_mount callbacks in registration order. Called by
/// the renderer (A3) after a batch of component views has been
/// appended to the tree. Safe to call when the queue is empty
/// (no-op).
pub fn flush_mounts() {
    // Drain the queue under a short borrow so callback bodies (which
    // may themselves register new on_mount) land in a fresh queue.
    let queue: Vec<Box<dyn FnOnce()>> = with_runtime(|rt| std::mem::take(&mut rt.pending_mounts));
    for cb in queue {
        cb();
    }
}

/// Look up the owners currently associated with `fn_ptr`. Used by the
/// A6 hot-reload path to find which live owners need disposal +
/// remount when subsecond patches a component function body. Returns
/// a snapshot — modifying the runtime's `component_owners` after
/// this call won't affect the returned `Vec`.
#[doc(hidden)]
pub fn owners_for_fn(fn_ptr: *const ()) -> Vec<OwnerId> {
    with_runtime(|rt| {
        rt.component_owners
            .get(&fn_ptr)
            .cloned()
            .unwrap_or_default()
    })
}
