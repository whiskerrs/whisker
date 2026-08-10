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
//! [`register_component`] so the hot-reload path can map
//! subsecond-patched fn pointers back to live owners.
//!
//! Lifecycle hooks:
//!
//! - [`on_mount`] — registered against the current owner; fires once
//!   on the next [`flush_mounts`], which runs after the component's
//!   view has been appended to its parent.
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
    // A component's reactive dependencies must come from the explicit
    // `effect` / `computed` calls inside its body. Without `untrack`, a
    // bare `signal.get()` in the body subscribes whatever outer node is
    // constructing us (a parent's `Show` effect, a route mount).
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
/// once on the next [`flush_mounts`] call, which runs after the
/// component's view is appended to its parent.
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

/// Run all queued on_mount callbacks in registration order. Called
/// after a batch of component views has been appended to the tree.
/// Safe to call when the queue is empty (no-op).
pub fn flush_mounts() {
    // Drained under a short borrow so callbacks that register their own
    // `on_mount` land in a fresh queue.
    let queue: Vec<Box<dyn FnOnce()>> = with_runtime(|rt| std::mem::take(&mut rt.pending_mounts));
    for cb in queue {
        // An `on_mount` may read signals to inspect post-mount state
        // but must never subscribe whatever node is on the stack at
        // drain time — an integration is free to call `flush_mounts`
        // from inside a reactive scope, so the queue enforces it.
        untrack(cb);
    }
}

/// Look up the owners currently associated with `fn_ptr` — which live
/// owners need disposal + remount when subsecond patches a component
/// function body. Returns a snapshot; later mutations of the runtime's
/// `component_owners` don't affect the returned `Vec`.
#[doc(hidden)]
pub fn owners_for_fn(fn_ptr: *const ()) -> Vec<Owner> {
    with_runtime(|rt| {
        rt.component_owners
            .get(&fn_ptr)
            .cloned()
            .unwrap_or_default()
    })
}

// Per-component remount. `mount_component_remountable` returns the
// body's root element directly — no wrapper element sits between a
// component body and its parent, so the Whisker component tree maps
// 1:1 onto the Lynx element tree.
//
// With no wrapper to serve as a stable placeholder, each mount's
// `(parent, previous_sibling)` is captured lazily: the mount stashes
// its `MountId` + body_root in `PENDING_MOUNT`, and `view::append_child`
// calls back through [`on_component_root_attached`] once that root is
// attached.
//
// Known limitations:
// - The anchor must outlive the remount. A sibling-managed component
//   that disposed itself between mount and patch leaves a stale anchor,
//   and remount falls back to the previous numeric position.
// - Component-local signal state is lost on remount; context-stored
//   state survives, its owners being above the disposed scope.
// - Props must implement `Clone` so the body closure can hand user code
//   fresh owned values on each invocation.

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
    /// Reads the component's props-layout hash *through subsecond
    /// dispatch* (the `#[component]` macro bakes the hash of the
    /// props signature into a generated fn). After a patch this
    /// returns the patch dylib's value.
    pub props_hash_fn: Rc<dyn Fn() -> u64 + 'static>,
    /// The layout hash as of when this site's `body` closure was
    /// created. If `props_hash_fn()` disagrees after a patch, the
    /// stored closure's captured environment no longer matches what
    /// the patched body code expects — re-running it would transmute
    /// across mismatched layouts (garbage props / UB), so the site
    /// must not be remounted in place.
    pub props_hash: u64,
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
        // Some other element; put the entry back so the body_root's
        // eventual `append_child` can still claim it.
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
/// element directly to the caller. No wrapper element is created, so
/// the Whisker component tree maps 1:1 with the Lynx element tree.
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
///
/// `props_hash_fn` reads the component's props-layout hash through
/// subsecond dispatch (see [`MountSite::props_hash_fn`]); the value
/// it returns *now* is recorded as the layout this site's `body`
/// closure was built against. Non-hot-reload callers (tests) can
/// pass `Box::new(|| 0)`.
pub fn mount_component_remountable<F>(
    fn_ptr: *const (),
    body: F,
    props_hash_fn: Box<dyn Fn() -> u64 + 'static>,
) -> Element
where
    F: Fn() -> Element + 'static,
{
    let body: Rc<dyn Fn() -> Element + 'static> = Rc::new(body);
    let props_hash_fn: Rc<dyn Fn() -> u64 + 'static> = Rc::from(props_hash_fn);
    let props_hash = props_hash_fn();

    let body_for_first = body.clone();
    let owner = Owner::new(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    // See `mount_component` for why the body runs untracked.
    let body_root = untrack(|| owner.with(|| (*body_for_first)()));

    // `parent` / `anchor` stay `None` until the next
    // `view::append_child` attaches `body_root` and calls back through
    // `on_component_root_attached`.
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
                props_hash_fn,
                props_hash,
            },
        );
        rt.fn_ptr_mounts.entry(fn_ptr).or_default().push(id);
        id
    });

    // The caller's `view::append_child(parent, body_root)` consumes
    // this and binds parent + anchor. An unconsumed predecessor (a body
    // whose root was never attached) is dropped here; its MountSite
    // stays parentless in the registry and remount lookups skip it.
    PENDING_MOUNT.with(|cell| cell.set(Some((mount_id, body_root))));

    body_root
}

/// Re-mount every remountable site whose `fn_ptr` is in the given
/// list. Called by the bootstrap's tick callback after a successful
/// subsecond patch.
///
/// The whole list is remounted as one batch: every old body root is
/// detached first, then each site's owner is disposed and its body
/// re-run, and finally the new roots are inserted at the indices their
/// old roots held. Non-remounted siblings never move, so sibling order
/// and their native state survive the patch.
///
/// Returns [`RemountStats`]: how many sites were remounted, and how
/// many were *refused* because their props layout changed. The
/// bootstrap escalates on either — `remounted == 0` means the patch had
/// no attached component to reflect through, and `layout_changed > 0`
/// means at least one stored body closure can no longer be re-run
/// safely.
pub fn remount_components_for(patched_fns: &[*const ()]) -> RemountStats {
    if patched_fns.is_empty() {
        return RemountStats::default();
    }
    // A site whose ancestor component is in the same batch is skipped:
    // remounting the ancestor rebuilds the whole subtree, so handling
    // the descendant separately either works on stale parent state or
    // no-ops against a cascade-disposed owner.
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

    if ids.is_empty() {
        return RemountStats::default();
    }

    // The batch must be one operation rather than a per-site loop:
    // a site's `anchor` is a sibling's body_root, so remounting siblings
    // one at a time detaches the anchors the later ones depend on and
    // they all fall back to index 0, clumping at the top of the parent
    // in hash-iteration order.

    struct RemountInfo {
        mount_id: MountId,
        parent: Element,
        old_body_root: Element,
        body: Rc<dyn Fn() -> Element + 'static>,
        fn_ptr: *const (),
        props_hash_fn: Rc<dyn Fn() -> u64 + 'static>,
        props_hash: u64,
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
                    props_hash_fn: site.props_hash_fn.clone(),
                    props_hash: site.props_hash,
                })
            })
            .collect()
    });

    // `props_hash_fn()` dispatches into the freshly applied patch and
    // returns the layout the *new* code was compiled for. A mismatch
    // against the site's stored hash means the patch changed the props
    // signature, and re-invoking the old closure would transmute across
    // mismatched capture layouts (garbage props / UB); the caller
    // escalates those to a full remount instead. Evaluated OUTSIDE
    // `with_runtime` — the hash getter re-enters subsecond dispatch.
    let (infos, layout_changed): (Vec<RemountInfo>, Vec<RemountInfo>) = infos
        .into_iter()
        .partition(|info| (info.props_hash_fn)() == info.props_hash);
    let layout_changed = layout_changed.len();

    if infos.is_empty() {
        return RemountStats {
            remounted: 0,
            layout_changed,
        };
    }

    // Snapshot each unique parent's child list before anything mutates.
    let mut parent_snapshot: std::collections::HashMap<Element, Vec<Element>> =
        std::collections::HashMap::new();
    for info in &infos {
        parent_snapshot
            .entry(info.parent)
            .or_insert_with(|| crate::view::children_of(info.parent));
    }

    // Detach every old body_root *before* any dispose runs.
    // `Owner::dispose` invalidates element handles, after which
    // `remove_child` silently no-ops against Lynx and the stale subtree
    // stays on screen.
    let mut by_parent: std::collections::HashMap<Element, Vec<(Element, Option<Element>)>> =
        std::collections::HashMap::new();
    for info in &infos {
        crate::view::remove_child(info.parent, info.old_body_root);
        by_parent
            .entry(info.parent)
            .or_default()
            .push((info.old_body_root, None));
    }

    let mut results: Vec<(MountId, Element, Element, Element, Owner)> =
        Vec::with_capacity(infos.len());
    for info in infos {
        let old_owner = with_runtime(|rt| {
            let site = rt.mount_sites.get_mut(&info.mount_id)?;
            site.body_root.take();
            site.owner.take()
        });
        // Remount runs with an empty owner stack; inherit the old
        // owner's parent or app-root contexts are lost.
        let parent_owner =
            old_owner.and_then(|o| with_runtime(|rt| rt.owners.get(o).and_then(|s| s.parent)));
        if let Some(o) = old_owner {
            o.dispose();
        }

        let new_owner = Owner::new(parent_owner);
        with_runtime(|rt| {
            if let Some(o) = rt.owners.get_mut(new_owner) {
                o.mount_fn = Some(info.fn_ptr);
            }
            rt.component_owners
                .entry(info.fn_ptr)
                .or_default()
                .push(new_owner);
        });
        // `untrack` so the body's signal reads register against its own
        // nested `effect`/`computed`s, not against whatever scheduler
        // context was active when the tick called into us.
        let new_body_root = untrack(|| new_owner.with(|| (*info.body)()));
        // The body's own `mount_component_remountable` calls leave a
        // PENDING_MOUNT entry behind, and nothing will consume it — the
        // batched path attaches roots via `insert_child_at`, not the
        // caller's `append_child`.
        PENDING_MOUNT.with(|cell| cell.set(None));

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

    for (parent, pairs) in &by_parent {
        let snapshot = parent_snapshot.get(parent).cloned().unwrap_or_default();
        let old_to_new: std::collections::HashMap<Element, Element> = pairs
            .iter()
            .filter_map(|(o, n)| n.map(|new_root| (*o, new_root)))
            .collect();

        // Snapshot with each old root replaced by its matching new one,
        // leaving non-replaced siblings untouched.
        let desired: Vec<Element> = snapshot
            .iter()
            .map(|c| old_to_new.get(c).copied().unwrap_or(*c))
            .collect();

        // Ascending order matters: inserting at index `i` shifts only
        // the elements from `i` onwards, so earlier placements stay put.
        let new_set: std::collections::HashSet<Element> =
            pairs.iter().filter_map(|(_, n)| *n).collect();
        for (idx, child) in desired.iter().enumerate() {
            if new_set.contains(child) {
                crate::view::insert_child_at(*parent, *child, idx);
            }
        }
    }

    for (mount_id, _, _, new_root, new_owner) in &results {
        with_runtime(|rt| {
            if let Some(site) = rt.mount_sites.get_mut(mount_id) {
                site.owner = Some(*new_owner);
                site.body_root = Some(*new_root);
            }
        });
    }

    // Refresh anchors from the now-final child order, or a future solo
    // patch of one of these siblings inherits a stale anchor and falls
    // back to index 0.
    for (mount_id, parent, _, new_root, _) in &results {
        let new_anchor = crate::view::previous_sibling(*parent, *new_root);
        with_runtime(|rt| {
            if let Some(site) = rt.mount_sites.get_mut(mount_id) {
                site.anchor = new_anchor;
            }
        });
    }

    RemountStats {
        remounted: results.len(),
        layout_changed,
    }
}

/// What [`remount_components_for`] did with one patch — the
/// bootstrap's full-remount escalation input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RemountStats {
    /// Mount sites disposed and re-mounted in place.
    pub remounted: usize,
    /// Mount sites *refused* because their props layout changed
    /// between the stored body closure and the patched code (see
    /// [`MountSite::props_hash`]). Non-zero means the on-screen
    /// subtree for those sites is stale and only a full remount can
    /// safely rebuild it.
    pub layout_changed: usize,
}
