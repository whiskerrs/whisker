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

use std::rc::Rc;

use super::owner::{create_owner, dispose_owner, with_owner};
use super::runtime::OwnerId;
use super::with_runtime;
use crate::view::{self, ElementHandle};

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

// ===========================================================================
// True per-component remount — wrapper-less (issue #17 / Y-2 P1)
// ===========================================================================
//
// `mount_component_remountable` runs the user's body inside a fresh
// owner and **returns the body's root element directly** — no wrapper
// `view` is inserted between the body and its parent. The Whisker
// component tree maps 1:1 with the Lynx element tree.
//
// To make remount still work without a wrapper as a stable
// placeholder, we capture each mount's `(parent, previous_sibling)`
// lazily: `mount_component_remountable` stashes the freshly-created
// `MountId` + body_root in a thread-local `PENDING_MOUNT` slot,
// and `view::append_child` (when it sees that body_root being
// attached) calls back via [`on_component_root_attached`] to
// populate `MountSite.parent` / `MountSite.anchor`.
//
// On a subsecond patch:
// 1. Look up the MountSite by patched fn_ptr.
// 2. Detach old body_root from parent (Whisker-side child mirror
//    keeps the position information so we know where to re-insert).
// 3. Dispose old owner — cascading reactive cleanup, on_cleanup,
//    nested component disposal.
// 4. Re-invoke body inside a fresh owner → new body_root.
// 5. Insert new body_root at the same slot (after the same
//    previous-sibling anchor, or at the start if no anchor).
//
// Trade-offs / known limitations:
// - The "previous sibling" anchor must remain alive across remounts.
//   If a sibling-managed component disposed itself between mount
//   and patch, the anchor is stale and remount falls back to
//   inserting at the previous numeric position (best effort).
//   For/Show interactions don't normally cause this because their
//   wrappers are themselves stable elements.
// - Component-local signal state is lost on remount; context-stored
//   state survives because its owners live above the disposed scope.
// - Props must implement `Clone` so the body closure can hand the
//   user code fresh owned values on each invocation.

use std::cell::Cell;

thread_local! {
    /// Set immediately before `mount_component_remountable` returns
    /// its body_root. Consumed by `view::append_child` on the next
    /// matching attach. The TLS is single-slot (last-writer-wins):
    /// nested component mounts handle themselves because the body's
    /// inner `view::append_child` calls drain the inner pending
    /// mounts before this function's own value is stashed.
    static PENDING_MOUNT: Cell<Option<(MountId, ElementHandle)>> = Cell::new(None);
}

/// Stable identifier for a remountable mount site. Generationless on
/// purpose — entries are removed when the site is torn down, so the
/// monotonic counter never collides for live entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MountId(pub(crate) u64);

/// One live remountable component mount.
pub(crate) struct MountSite {
    /// Function pointer of the component fn that produced this mount.
    /// Used for the patched-fn lookup at hot-reload time.
    pub fn_ptr: *const (),
    /// User body closure. `Rc` so the remount path can clone the
    /// handle out of the runtime borrow before invoking it (the body
    /// re-enters the runtime via `view::*` / `signal()` / etc., so
    /// holding the runtime borrow across the call would deadlock).
    pub body: Rc<dyn Fn() -> ElementHandle + 'static>,
    /// Current owner — `Some` between mounts, `None` during the
    /// dispose-then-remount window.
    pub owner: Option<OwnerId>,
    /// Element handle the body returned for its outermost element.
    /// Detached from the parent at the start of each remount, then
    /// replaced by the new body's root inserted at the same slot.
    pub body_root: Option<ElementHandle>,
    /// Parent element this component is attached to. `None` until
    /// `view::append_child` fires for the body_root for the first
    /// time. `Some(_)` thereafter, kept up to date across remounts.
    pub parent: Option<ElementHandle>,
    /// Element handle that was the body_root's immediate predecessor
    /// in `parent`'s child list at attach time. `None` if the body
    /// was the first child of parent. Stable across remounts unless
    /// the anchor itself is removed by some other code path.
    pub anchor: Option<ElementHandle>,
}

/// Called by `view::append_child` after every successful attach.
/// If there's a pending component mount whose body_root matches the
/// just-attached `child`, finalise its MountSite by recording the
/// parent + previous-sibling anchor.
///
/// No-op if no mount is pending or the pending body_root doesn't
/// match — in that case the pending entry is restored so a later
/// matching attach can still claim it.
pub fn on_component_root_attached(parent: ElementHandle, child: ElementHandle) {
    let pending = PENDING_MOUNT.with(|cell| cell.take());
    let Some((mount_id, root)) = pending else {
        return;
    };
    if root != child {
        // The attach was for some other element. Put the pending
        // entry back so the body_root's eventual `append_child`
        // can still pick it up.
        PENDING_MOUNT.with(|cell| cell.set(Some((mount_id, root))));
        return;
    }
    let anchor = crate::view::previous_sibling(parent, child);
    super::with_runtime(|rt| {
        if let Some(site) = rt.mount_sites.get_mut(&mount_id) {
            site.parent = Some(parent);
            site.anchor = anchor;
        }
    });
}

/// Test/internal: clear the pending-mount slot. Use between
/// scenarios that share a thread.
#[doc(hidden)]
pub fn __reset_pending_mount_for_tests() {
    PENDING_MOUNT.with(|cell| cell.set(None));
}

/// Mount a component with full remount support — wrapper-less.
///
/// Runs `body` inside a fresh owner and returns the body's root
/// element directly to the caller. No wrapper element is created,
/// so the Whisker component tree maps 1:1 with the Lynx element
/// tree (issue #17).
///
/// To make remount work without a stable wrapper handle in the
/// parent's child list, the function stashes a pending-mount entry
/// in a thread-local just before returning. The next
/// [`view::append_child`] call that sees this body_root being
/// attached finalises the MountSite (recording parent + previous
/// sibling). The [`on_component_root_attached`] callback handles
/// that side of the handshake.
///
/// On a subsecond patch matching `fn_ptr`, the runtime calls
/// [`remount_components_for`] which disposes the current owner,
/// re-invokes `body` in a new owner, removes the old body_root
/// from its parent, and inserts the new body_root at the same slot
/// (using the recorded anchor).
pub fn mount_component_remountable<F>(fn_ptr: *const (), body: F) -> ElementHandle
where
    F: Fn() -> ElementHandle + 'static,
{
    let body: Rc<dyn Fn() -> ElementHandle + 'static> = Rc::new(body);

    // Initial mount: fresh owner, run body, capture root.
    let body_for_first = body.clone();
    let owner = create_owner(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    let body_root = with_owner(owner, || (*body_for_first)());

    // Register the MountSite with parent / anchor as `None` for now
    // — the next `view::append_child` that attaches `body_root`
    // will populate them via `on_component_root_attached`.
    let mount_id = with_runtime(|rt| {
        rt.mount_id_counter += 1;
        let id = MountId(rt.mount_id_counter);
        rt.mount_sites.insert(
            id,
            MountSite {
                fn_ptr,
                body,
                owner: Some(owner),
                body_root: Some(body_root),
                parent: None,
                anchor: None,
            },
        );
        rt.fn_ptr_mounts.entry(fn_ptr).or_default().push(id);
        id
    });

    // Hand the (MountId, body_root) pair to the pending slot. The
    // caller's `view::append_child(parent, body_root)` consumes it
    // and binds parent + anchor. Any previously-stashed pending
    // mount that *wasn't* consumed (orphaned — body returned a root
    // that was never attached) gets dropped here; the orphan's
    // MountSite stays in the registry without a parent and will
    // simply be skipped by remount lookups.
    PENDING_MOUNT.with(|cell| cell.set(Some((mount_id, body_root))));

    body_root
}

/// Re-mount every remountable site whose `fn_ptr` is in the given
/// list. Called by the bootstrap's tick callback after a successful
/// subsecond patch. Internally:
///
/// 1. Collect the set of `MountId`s to remount (deduplicated, even
///    if the patch list contains the same fn pointer multiple times).
/// 2. For each: detach the previous body root from its wrapper,
///    dispose the previous owner (cascading reactive cleanup), then
///    create a fresh owner, re-invoke the body, append the new root
///    to the same wrapper, and update the site's `owner` / `body_root`.
///
/// The wrapper element stays put in the parent's child list across
/// the whole flow, so the user-visible navigation / scroll position
/// / sibling order are preserved.
pub fn remount_components_for(patched_fns: &[*const ()]) {
    if patched_fns.is_empty() {
        return;
    }
    // Collect candidate mount sites, then filter out any whose
    // ancestor component is also in this patch batch. When a
    // parent component's body is patched, remounting it
    // re-creates the whole subtree from scratch — separately
    // remounting children would either operate on stale parent
    // state (if processed first) or no-op (if scrubbed by the
    // cascading dispose). Both outcomes are wrong; skipping the
    // descendant entirely is the correct semantics.
    let patched_set: std::collections::HashSet<*const ()> =
        patched_fns.iter().copied().collect();
    let ids: Vec<MountId> = with_runtime(|rt| {
        let mut candidates: Vec<MountId> = Vec::new();
        for fp in patched_fns {
            if let Some(list) = rt.fn_ptr_mounts.get(fp) {
                for id in list {
                    if !candidates.contains(id) {
                        candidates.push(*id);
                    }
                }
            }
        }
        candidates
            .into_iter()
            .filter(|mount_id| {
                // Walk the owner chain upward; if any ancestor
                // owner's mount_fn is in `patched_set`, skip.
                let site = match rt.mount_sites.get(mount_id) {
                    Some(s) => s,
                    None => return false,
                };
                let mut cursor = match site.owner {
                    Some(o) => o,
                    None => return false,
                };
                while let Some(parent) = rt.owners.get(cursor).and_then(|o| o.parent) {
                    if let Some(mf) = rt.owners.get(parent).and_then(|o| o.mount_fn) {
                        if patched_set.contains(&mf) {
                            return false;
                        }
                    }
                    cursor = parent;
                }
                true
            })
            .collect()
    });

    for mount_id in ids {
        remount_one(mount_id);
    }
}

fn remount_one(mount_id: MountId) {
    // Step 1: pull parent / anchor / body Rc / previous owner+root
    // out of the runtime. We can't hold the borrow across `body()`
    // because user code inside it re-enters via `view::*` /
    // `signal()` etc.
    let (parent, anchor, body, old_owner, old_body_root, fn_ptr) = match with_runtime(|rt| {
        let site = rt.mount_sites.get_mut(&mount_id)?;
        let body = site.body.clone();
        let owner = site.owner.take();
        let body_root = site.body_root.take();
        Some((
            site.parent,
            site.anchor,
            body,
            owner,
            body_root,
            site.fn_ptr,
        ))
    }) {
        Some(t) => t,
        None => return,
    };

    // If the mount never finished binding to a parent (orphaned —
    // body root never went through `append_child`), there's nowhere
    // to remount it. Skip; the entry stays in the registry but is
    // effectively dead.
    let Some(parent) = parent else {
        return;
    };

    // Step 2: figure out where the old body root sits in `parent`'s
    // child list, so we can re-insert at the same position. We
    // prefer the recorded anchor (stable across sibling churn) and
    // fall back to the body_root's current numeric position if the
    // anchor was removed in the meantime.
    let insert_index: usize = if let Some(a) = anchor {
        crate::view::child_index(parent, a)
            .map(|i| i + 1)
            .unwrap_or(0)
    } else if let Some(r) = old_body_root {
        crate::view::child_index(parent, r).unwrap_or(0)
    } else {
        0
    };

    // Step 3: detach the old body root from parent.
    if let Some(root) = old_body_root {
        view::remove_child(parent, root);
    }

    // Step 4: dispose the old owner, cascading reactive cleanup +
    // running any registered `on_cleanup` callbacks. This also
    // pulls the old owner out of `component_owners` (the existing
    // `dispose_owner` scrub logic).
    if let Some(o) = old_owner {
        dispose_owner(o);
    }

    // Step 5: create a fresh owner and re-invoke the body inside it.
    let new_owner = create_owner(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(new_owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(new_owner);
    });
    let new_body_root = with_owner(new_owner, || (*body)());

    // Step 6: insert new body_root at the slot's original index,
    // clear the pending-mount entry the body's
    // `mount_component_remountable` call left behind (we're not
    // going through the normal "caller does append_child" path
    // here; we're inserting directly), and update the mount site.
    PENDING_MOUNT.with(|cell| cell.set(None));
    view::insert_child_at(parent, new_body_root, insert_index);

    with_runtime(|rt| {
        if let Some(site) = rt.mount_sites.get_mut(&mount_id) {
            site.owner = Some(new_owner);
            site.body_root = Some(new_body_root);
            // parent and anchor are unchanged (we re-inserted at
            // the same logical slot).
        }
    });
}
