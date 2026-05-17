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
// True per-component remount (PR #15 enhancement)
// ===========================================================================
//
// `mount_component_remountable` wraps the user's body in a permanent
// `view` element ("wrapper") that survives across remounts. The body
// closure (`Rc<dyn Fn() -> ElementHandle>`) is stored in a side table
// keyed by a stable `MountId`. On a subsecond patch, the runtime
// walks the patched fn pointers, finds the matching mount sites,
// detaches the previous body root from the wrapper, disposes the
// component owner (cascading cleanups), re-invokes the body closure
// inside a fresh owner, and re-attaches the new body root to the
// wrapper. The parent's child list is unchanged throughout — only
// the wrapper's interior swaps.
//
// Trade-offs documented in PR #15:
// - Adds one wrapper `view` per `#[component]` (extra layer in the
//   element tree; typically invisible to Flexbox layouts but worth
//   noting for tight CSS).
// - Component-local signal state is lost on remount; context-stored
//   state survives because its owners live above the disposed scope.
// - Props must implement `Clone` so the body closure can hand the
//   user code fresh owned values on each invocation. The
//   `#[component]` macro emits `let prop = prop_capture.clone();`
//   inside the body, so `Copy` types pay no cost and `Clone` types
//   clone once per remount (never during normal operation).

/// Stable identifier for a remountable mount site. Generationless on
/// purpose — entries are removed when the site is torn down, so the
/// monotonic counter never collides for live entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MountId(pub(crate) u64);

/// One live remountable component mount.
pub(crate) struct MountSite {
    /// The wrapper `view` element this mount lives inside. Visible to
    /// the parent's `append_child`; stable across remounts.
    pub wrapper: ElementHandle,
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
    /// Detached from `wrapper` at the start of each remount, then
    /// replaced by the new body's root.
    pub body_root: Option<ElementHandle>,
}

/// Mount a component with full remount support.
///
/// Creates a permanent `view` wrapper, runs `body` inside a fresh
/// owner, appends the body's root to the wrapper, and registers a
/// `MountSite` keyed by a fresh `MountId`. Returns the wrapper —
/// callers (typically the `#[component]` proc-macro) treat it as
/// the component's output, and the parent's `render!` attaches it
/// the same way it would any other child element.
///
/// On a subsecond patch matching `fn_ptr`, the runtime calls
/// [`remount_components_for`] (via the bootstrap tick callback)
/// which disposes the current owner, re-invokes `body` in a new
/// owner, and re-attaches the result to the still-living wrapper.
/// The parent's child list is untouched.
pub fn mount_component_remountable<F>(fn_ptr: *const (), body: F) -> ElementHandle
where
    F: Fn() -> ElementHandle + 'static,
{
    let wrapper = view::create_element(crate::element::ElementTag::View);
    // The wrapper is a remount placeholder — Whisker needs a stable
    // handle to swap the body root underneath on hot-reload patches.
    // But it must not participate in layout, otherwise the
    // unstyled view collapses inside flex parents (Lynx's view
    // default is `flex-direction: row; content-fit`, which
    // squeezes the body's flex sizing to nothing).
    //
    // `display: contents` is the CSS feature designed for exactly
    // this — the element exists in the element tree but contributes
    // no layout box; its children inherit the parent's slot for
    // layout purposes (flex / grid / alignment).
    //
    // If Lynx doesn't honour `display: contents` we'll see the same
    // layout breakage `#[component]` showed before this line; fall
    // back to the marker-elements approach (see issue #17).
    view::set_inline_styles(wrapper, "display: contents;");
    let body: Rc<dyn Fn() -> ElementHandle + 'static> = Rc::new(body);

    // Initial mount: fresh owner, run body, attach to wrapper.
    let body_for_first = body.clone();
    let owner = create_owner(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(owner);
    });
    let body_root = with_owner(owner, || (*body_for_first)());
    view::append_child(wrapper, body_root);

    // Stash the site for later remount. We don't add the wrapper to
    // any owner's element list — its lifetime is managed by the
    // parent, since the parent's `append_child(parent, wrapper)`
    // will keep it visible until the parent dropping detaches it.
    let mount_id = with_runtime(|rt| {
        rt.mount_id_counter += 1;
        let id = MountId(rt.mount_id_counter);
        rt.mount_sites.insert(
            id,
            MountSite {
                wrapper,
                fn_ptr,
                body,
                owner: Some(owner),
                body_root: Some(body_root),
            },
        );
        rt.fn_ptr_mounts.entry(fn_ptr).or_default().push(id);
        id
    });
    // Suppress unused — `mount_id` is the canonical identifier; we
    // keep the binding so future debugging / tracing has a hook,
    // and so the mount-site insertion above isn't optimised out.
    let _ = mount_id;

    wrapper
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
    let ids: Vec<MountId> = with_runtime(|rt| {
        let mut out: Vec<MountId> = Vec::new();
        for fp in patched_fns {
            if let Some(list) = rt.fn_ptr_mounts.get(fp) {
                for id in list {
                    if !out.contains(id) {
                        out.push(*id);
                    }
                }
            }
        }
        out
    });

    for mount_id in ids {
        remount_one(mount_id);
    }
}

fn remount_one(mount_id: MountId) {
    // Step 1: pull wrapper + body Rc + previous owner/root out of
    // the runtime. We can't hold the borrow across `body()` because
    // user code inside it re-enters via `view::*` / `signal()` etc.
    let (wrapper, body, old_owner, old_body_root, fn_ptr) = match with_runtime(|rt| {
        let site = rt.mount_sites.get_mut(&mount_id)?;
        let body = site.body.clone();
        let owner = site.owner.take();
        let body_root = site.body_root.take();
        Some((site.wrapper, body, owner, body_root, site.fn_ptr))
    }) {
        Some(t) => t,
        None => return,
    };

    // Step 2: detach the old body root from the wrapper (the new
    // root will land in the same slot when we re-append below).
    if let Some(root) = old_body_root {
        view::remove_child(wrapper, root);
    }

    // Step 3: dispose the old owner, cascading reactive cleanup +
    // running any registered `on_cleanup` callbacks. This also
    // pulls the old owner out of `component_owners` (the existing
    // `dispose_owner` scrub logic).
    if let Some(o) = old_owner {
        dispose_owner(o);
    }

    // Step 4: create a fresh owner and re-invoke the body inside it.
    let new_owner = create_owner(None);
    with_runtime(|rt| {
        if let Some(o) = rt.owners.get_mut(new_owner) {
            o.mount_fn = Some(fn_ptr);
        }
        rt.component_owners.entry(fn_ptr).or_default().push(new_owner);
    });
    let new_body_root = with_owner(new_owner, || (*body)());

    // Step 5: re-attach to the wrapper and update the mount site.
    view::append_child(wrapper, new_body_root);
    with_runtime(|rt| {
        if let Some(site) = rt.mount_sites.get_mut(&mount_id) {
            site.owner = Some(new_owner);
            site.body_root = Some(new_body_root);
        }
    });
}
