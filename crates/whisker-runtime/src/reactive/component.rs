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

use super::runtime::Owner;
use super::{untrack, with_runtime};
use crate::view::Element;

/// Mount a component: create a fresh child owner, register `fn_ptr`
/// against it for hot reload, run `body` inside that owner, and
/// return both the owner id and the body's result.
///
/// The caller is responsible for keeping the returned `Owner` alive
/// (e.g. attaching it to the parent component's owner-children list
/// via the renderer) and for disposing it when the component
/// unmounts. The owner is already linked as a child of the
/// current-owner-at-call-time, so calling [`Owner::dispose`] on an
/// ancestor will cascade.
pub fn mount_component<R>(fn_ptr: *const (), body: impl FnOnce() -> R) -> (Owner, R) {
    let owner = Owner::new(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    // Component bodies build a static Element tree; the reactive
    // dependencies they declare must come from explicit
    // `effect` / `computed` calls *inside* the body, not from
    // ambient signal reads contaminating whatever outer reactive
    // node we happened to be constructed inside (a parent
    // component's `Show` effect, `StackLayout`'s route mount, etc.).
    // Clear the tracker around the body call so a direct
    // `signal.get()` in user code doesn't silently subscribe the
    // outer node.
    let result = untrack(|| owner.with(body));
    (owner, result)
}

/// Dispose a component owner *and* deregister it from
/// `component_owners`. Use this instead of plain `Owner::dispose` for
/// owners created via `mount_component`.
pub fn unmount_component(owner: Owner) {
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
    owner.dispose();
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
        // `on_mount` callbacks are fire-once side effects that may
        // read signals to inspect post-mount state but should never
        // subscribe whatever node happens to be on the call stack
        // when the queue gets drained. In production `flush_mounts`
        // runs after `reactive_flush` returns (tracker already
        // cleared by the scheduler), but other integrations may
        // call it from inside a reactive scope — wrap each `cb` in
        // `untrack` so the invariant is enforced by the queue itself.
        untrack(cb);
    }
}

/// Look up the owners currently associated with `fn_ptr`. Used by the
/// A6 hot-reload path to find which live owners need disposal +
/// remount when subsecond patches a component function body. Returns
/// a snapshot — modifying the runtime's `component_owners` after
/// this call won't affect the returned `Vec`.
#[doc(hidden)]
pub fn owners_for_fn(fn_ptr: *const ()) -> Vec<Owner> {
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
    static PENDING_MOUNT: Cell<Option<(MountId, Element)>> = const { Cell::new(None) };
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
    pub body: Rc<dyn Fn() -> Element + 'static>,
    /// Current owner — `Some` between mounts, `None` during the
    /// dispose-then-remount window.
    pub owner: Option<Owner>,
    /// Element handle the body returned for its outermost element.
    /// Detached from the parent at the start of each remount, then
    /// replaced by the new body's root inserted at the same slot.
    pub body_root: Option<Element>,
    /// Parent element this component is attached to. `None` until
    /// `view::append_child` fires for the body_root for the first
    /// time. `Some(_)` thereafter, kept up to date across remounts.
    pub parent: Option<Element>,
    /// Element handle that was the body_root's immediate predecessor
    /// in `parent`'s child list at attach time. `None` if the body
    /// was the first child of parent. Stable across remounts unless
    /// the anchor itself is removed by some other code path.
    pub anchor: Option<Element>,
    /// True when this component's body_root was installed as the page
    /// root via `view::set_root` (a top-level `#[whisker::main]`
    /// component) rather than attached under a parent. Remount re-installs
    /// it with `view::set_root` instead of inserting into a parent.
    pub is_root: bool,
}

/// Called by `view::append_child` after every successful attach.
/// If there's a pending component mount whose body_root matches the
/// just-attached `child`, finalise its MountSite by recording the
/// parent + previous-sibling anchor.
///
/// No-op if no mount is pending or the pending body_root doesn't
/// match — in that case the pending entry is restored so a later
/// matching attach can still claim it.
pub fn on_component_root_attached(parent: Element, child: Element) {
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

/// Called by `view::set_root` after the page root is installed.
/// If a pending component mount's body_root matches `page`, this is a
/// top-level component (`#[whisker::main] fn app() { render!{ Root } }`):
/// mark its MountSite `is_root` so `remount_components_for` re-installs it
/// via `set_root` on hot patch. No-op (and restores the pending entry) if
/// nothing pending or the body_root doesn't match.
pub fn on_component_root_set(page: Element) {
    let pending = PENDING_MOUNT.with(|cell| cell.take());
    let Some((mount_id, root)) = pending else {
        return;
    };
    if root != page {
        PENDING_MOUNT.with(|cell| cell.set(Some((mount_id, root))));
        return;
    }
    super::with_runtime(|rt| {
        if let Some(site) = rt.mount_sites.get_mut(&mount_id) {
            site.is_root = true;
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
pub fn mount_component_remountable<F>(fn_ptr: *const (), body: F) -> Element
where
    F: Fn() -> Element + 'static,
{
    let body: Rc<dyn Fn() -> Element + 'static> = Rc::new(body);

    // Initial mount: fresh owner, run body, capture root.
    let body_for_first = body.clone();
    let owner = Owner::new(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    // See `mount_component` for the rationale on the `untrack`
    // bracket. Same invariant applies to the remountable variant.
    let body_root = untrack(|| owner.with(|| (*body_for_first)()));

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
                is_root: false,
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
    let patched_set: std::collections::HashSet<*const ()> = patched_fns.iter().copied().collect();
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

    // Root sites (top-level components installed via `set_root`) have no
    // parent in the child mirror, so the batched insert path can't handle
    // them. Re-install each via `set_root` with a fresh owner + new body.
    let (root_ids, ids): (Vec<MountId>, Vec<MountId>) = ids.into_iter().partition(|mid| {
        with_runtime(|rt| rt.mount_sites.get(mid).map(|s| s.is_root).unwrap_or(false))
    });
    for mid in root_ids {
        let Some((body, fn_ptr)) = with_runtime(|rt| {
            let site = rt.mount_sites.get(&mid)?;
            Some((site.body.clone(), site.fn_ptr))
        }) else {
            continue;
        };
        // Dispose the old owner (cascading reactive cleanup) before re-running.
        let old_owner = with_runtime(|rt| {
            let site = rt.mount_sites.get_mut(&mid)?;
            site.body_root.take();
            site.owner.take()
        });
        if let Some(o) = old_owner {
            o.dispose();
        }
        let new_owner = Owner::new(None);
        with_runtime(|rt| {
            if let Some(o) = rt.owners.get_mut(new_owner) {
                o.mount_fn = Some(fn_ptr);
            }
            rt.component_owners
                .entry(fn_ptr)
                .or_default()
                .push(new_owner);
        });
        let new_body_root = untrack(|| new_owner.with(|| (*body)()));
        // The body's nested `mount_component_remountable` calls leave a
        // PENDING_MOUNT behind; drain it — we re-install via `set_root`,
        // not the caller's `append_child`.
        PENDING_MOUNT.with(|cell| cell.set(None));
        crate::view::set_root(new_body_root);
        with_runtime(|rt| {
            if let Some(site) = rt.mount_sites.get_mut(&mid) {
                site.owner = Some(new_owner);
                site.body_root = Some(new_body_root);
                // is_root stays true.
            }
        });
    }

    if ids.is_empty() {
        return;
    }

    // ---- Batched remount that preserves sibling order ---------------------
    //
    // The naive "one-at-a-time" version (`remount_one` per site) suffers
    // anchor staleness when sibling components are remounted together:
    // each site's `anchor` is a sibling's body_root, and once that
    // sibling has been remounted earlier in the loop, the anchor points
    // at an element that has already been detached → fallback to
    // index 0 → siblings clump at the top of the parent in
    // hash-iteration order, visibly scrambling the layout.
    //
    // Instead we do the whole batch as one operation:
    //   1. Snapshot each unique parent's current child list before
    //      anything mutates.
    //   2. For every site, dispose old owner + run new body to get the
    //      new body_root. The new body runs against a fresh owner so
    //      reactive state is isolated. None of this touches the parent's
    //      child list.
    //   3. For each parent, build the desired final child list by
    //      replacing each old body_root with its new body_root, leaving
    //      non-replaced siblings untouched.
    //   4. Remove every old body_root from the parent, then re-insert
    //      each new body_root at its desired index (ascending order).
    //   5. Refresh anchors from the post-mutation child list so future
    //      individual remounts also see a coherent state.

    struct RemountInfo {
        mount_id: MountId,
        parent: Element,
        old_body_root: Element,
        body: Rc<dyn Fn() -> Element + 'static>,
        fn_ptr: *const (),
    }

    let infos: Vec<RemountInfo> = with_runtime(|rt| {
        ids.iter()
            .filter_map(|mid| {
                let site = rt.mount_sites.get(mid)?;
                Some(RemountInfo {
                    mount_id: *mid,
                    parent: site.parent?,
                    old_body_root: site.body_root?,
                    body: site.body.clone(),
                    fn_ptr: site.fn_ptr,
                })
            })
            .collect()
    });

    if infos.is_empty() {
        return;
    }

    // 1. Snapshot each unique parent's child list.
    let mut parent_snapshot: std::collections::HashMap<Element, Vec<Element>> =
        std::collections::HashMap::new();
    for info in &infos {
        parent_snapshot
            .entry(info.parent)
            .or_insert_with(|| crate::view::children_of(info.parent));
    }

    // 2. Detach every old body_root from its parent *before* any
    //    dispose runs. Element handles get invalidated by
    //    `Owner::dispose` (renderer slot becomes `None`), so once
    //    disposed, subsequent `remove_child` calls would silently
    //    no-op against Lynx — visible as "stale subtree still on
    //    screen" after hot reload. Doing the remove first keeps the
    //    handle live.
    let mut by_parent: std::collections::HashMap<Element, Vec<(Element, Option<Element>)>> =
        std::collections::HashMap::new();
    for info in &infos {
        crate::view::remove_child(info.parent, info.old_body_root);
        by_parent
            .entry(info.parent)
            .or_default()
            .push((info.old_body_root, None));
    }

    // 3. Dispose old owners + run new bodies, collecting (mount_id,
    //    parent, old_root, new_root, new_owner).
    let mut results: Vec<(MountId, Element, Element, Element, Owner)> =
        Vec::with_capacity(infos.len());
    for info in infos {
        let old_owner = with_runtime(|rt| {
            let site = rt.mount_sites.get_mut(&info.mount_id)?;
            site.body_root.take();
            site.owner.take()
        });
        if let Some(o) = old_owner {
            o.dispose();
        }

        let new_owner = Owner::new(None);
        with_runtime(|rt| {
            if let Some(o) = rt.owners.get_mut(new_owner) {
                o.mount_fn = Some(info.fn_ptr);
            }
            rt.component_owners
                .entry(info.fn_ptr)
                .or_default()
                .push(new_owner);
        });
        // `untrack` so the remounted body's signal reads register
        // against its own nested `effect`/`computed`s, not against
        // whatever scheduler context happens to be active when
        // `tick_callback` calls into us.
        let new_body_root = untrack(|| new_owner.with(|| (*info.body)()));
        // The body's `mount_component_remountable` calls leave a
        // PENDING_MOUNT entry behind; we drain it here because the
        // batched path attaches the new root via `insert_child_at`
        // directly, not via the caller's `append_child`.
        PENDING_MOUNT.with(|cell| cell.set(None));

        // Backfill the new_root into by_parent so step 4 can map
        // old → new when computing the desired final order.
        if let Some(list) = by_parent.get_mut(&info.parent) {
            if let Some(entry) = list
                .iter_mut()
                .find(|(o, n)| *o == info.old_body_root && n.is_none())
            {
                entry.1 = Some(new_body_root);
            }
        }

        results.push((
            info.mount_id,
            info.parent,
            info.old_body_root,
            new_body_root,
            new_owner,
        ));
    }

    // 4. Per-parent: compute desired final order, insert new roots
    //    at their target indices. (Removes already happened in
    //    step 2 — the live-handle requirement.)
    for (parent, pairs) in &by_parent {
        let snapshot = parent_snapshot.get(parent).cloned().unwrap_or_default();
        let old_to_new: std::collections::HashMap<Element, Element> = pairs
            .iter()
            .filter_map(|(o, n)| n.map(|new_root| (*o, new_root)))
            .collect();

        // Desired final list = snapshot with each old replaced by its
        // matching new (leaving non-replaced siblings untouched).
        let desired: Vec<Element> = snapshot
            .iter()
            .map(|c| old_to_new.get(c).copied().unwrap_or(*c))
            .collect();

        // Insert new body_roots at their desired indices in ascending
        // order. Non-replaced siblings remain in place; inserting at
        // index `i` only shifts elements from `i` onwards by one slot,
        // which is exactly the semantics we want.
        let new_set: std::collections::HashSet<Element> =
            pairs.iter().filter_map(|(_, n)| *n).collect();
        for (idx, child) in desired.iter().enumerate() {
            if new_set.contains(child) {
                crate::view::insert_child_at(*parent, *child, idx);
            }
        }
    }

    // 4. Update each MountSite to point at its new owner + new root.
    for (mount_id, _, _, new_root, new_owner) in &results {
        with_runtime(|rt| {
            if let Some(site) = rt.mount_sites.get_mut(mount_id) {
                site.owner = Some(*new_owner);
                site.body_root = Some(*new_root);
            }
        });
    }

    // 5. Refresh anchors based on the now-final parent children
    //    layout — otherwise a *future* solo patch of one of these
    //    siblings would inherit a stale anchor and fall back to
    //    index 0 again.
    for (mount_id, parent, _, new_root, _) in &results {
        let new_anchor = crate::view::previous_sibling(*parent, *new_root);
        with_runtime(|rt| {
            if let Some(site) = rt.mount_sites.get_mut(mount_id) {
                site.anchor = new_anchor;
            }
        });
    }
}
