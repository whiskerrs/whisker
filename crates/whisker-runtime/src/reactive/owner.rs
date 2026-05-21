//! Owner (scope) lifecycle: create, dispose, enter/exit.
//!
//! Owners form a tree. Every reactive primitive (signal / effect /
//! computed) belongs to exactly one owner. Disposing an owner cascades
//! into its children, then frees every node it allocated, then runs
//! its cleanup callbacks in LIFO order.
//!
//! Users normally don't call these directly — they use
//! `#[component]`, `provide_context`, `on_cleanup` etc. — but tests
//! and the renderer machinery do.

use super::runtime::{Owner, OwnerId};
use super::with_runtime;

/// Create a new owner. If `parent` is `None` the current top-of-stack
/// owner is used (or the owner becomes a root if the stack is empty).
/// Returns the new owner's id.
pub fn create_owner(parent: Option<OwnerId>) -> OwnerId {
    with_runtime(|rt| {
        let parent = parent.or_else(|| rt.current_owner());
        let id = rt.owners.insert(Owner::new(parent));
        if let Some(p) = parent {
            if let Some(parent_owner) = rt.owners.get_mut(p) {
                parent_owner.children.push(id);
            }
        }
        id
    })
}

/// Push `owner` as the current scope, run `f`, pop back. Reactive
/// primitives allocated inside `f` will belong to this owner.
pub fn with_owner<R>(owner: OwnerId, f: impl FnOnce() -> R) -> R {
    with_runtime(|rt| rt.owner_stack.push(owner));
    let result = f();
    with_runtime(|rt| {
        let popped = rt.owner_stack.pop();
        debug_assert_eq!(
            popped,
            Some(owner),
            "with_owner: stack imbalance — owner pop didn't match push"
        );
    });
    result
}

/// Dispose `owner`, freeing all its descendants, nodes, and running
/// its cleanup callbacks.
///
/// Recursive — disposes children first, then this owner. Safe to call
/// even if the owner has already been disposed (no-op).
pub fn dispose_owner(owner: OwnerId) {
    // Step 1: collect what needs cleaning. We pull data out of the
    // runtime in a short borrow rather than holding it through the
    // recursion, because each level may itself need to mutate the
    // runtime (running cleanup callbacks does not, but symmetrically
    // we keep the pattern simple by avoiding nested borrows).
    let children;
    let nodes;
    let cleanups;
    let parent;
    let mount_fn;
    let elements;
    {
        let removed = with_runtime(|rt| rt.owners.remove(owner));
        let Some(o) = removed else { return };
        children = o.children;
        nodes = o.nodes;
        cleanups = o.cleanups;
        parent = o.parent;
        mount_fn = o.mount_fn;
        elements = o.elements;
    }

    // Step 1b: if this was a component owner, scrub the hot-reload
    // registry so the fn pointer doesn't list a freed slot. Without
    // this, A6's `owners_for_fn` would return a dangling OwnerId
    // and remount logic would fault.
    if let Some(fp) = mount_fn {
        with_runtime(|rt| {
            if let Some(list) = rt.component_owners.get_mut(&fp) {
                list.retain(|o| *o != owner);
                if list.is_empty() {
                    rt.component_owners.remove(&fp);
                }
            }
        });

        // Step 1c: also clean up any remountable MountSite whose
        // owner is this one. Without this scrub, cascading disposal
        // (e.g. parent component re-mounts and discards its
        // sub-tree) leaves orphan MountSites behind, and the next
        // `remount_components_for` call processes them — operating
        // on freed parent / body_root handles, with visible
        // corruption (issue #17 follow-up).
        //
        // `site.owner` is `None` *during* a remount (the takes-then-
        // reinstalls window in `remount_one`), so this scan won't
        // accidentally evict the site that's mid-flight. It only
        // matches MountSites whose component owner is the one
        // actually being disposed.
        with_runtime(|rt| {
            let stale: Vec<super::component::MountId> = rt
                .mount_sites
                .iter()
                .filter_map(|(id, site)| {
                    if site.owner == Some(owner) {
                        Some(*id)
                    } else {
                        None
                    }
                })
                .collect();
            for id in stale {
                rt.mount_sites.remove(&id);
                if let Some(list) = rt.fn_ptr_mounts.get_mut(&fp) {
                    list.retain(|m| *m != id);
                    if list.is_empty() {
                        rt.fn_ptr_mounts.remove(&fp);
                    }
                }
            }
        });
    }

    // Step 2: detach from parent's children list.
    if let Some(p) = parent {
        with_runtime(|rt| {
            if let Some(parent_owner) = rt.owners.get_mut(p) {
                parent_owner.children.retain(|&c| c != owner);
            }
        });
    }

    // Step 3: dispose descendants (post-order — bottom up).
    for child in children {
        dispose_owner(child);
    }

    // Step 4: free every node this owner allocated. For effects /
    // computed values, also detach them from any subscriber list they were on,
    // so other live nodes don't try to notify a freed slot later.
    with_runtime(|rt| {
        for node_id in &nodes {
            let Some(node) = rt.nodes.remove(*node_id) else {
                continue;
            };
            // Remove ourselves from every source's subscriber list.
            for source in node.sources {
                if let Some(src_node) = rt.nodes.get_mut(source) {
                    src_node.subscribers.remove(node_id);
                }
            }
            // Remove ourselves from every subscriber's source list —
            // a signal we owned may have been read by an outer effect.
            for sub in node.subscribers {
                if let Some(sub_node) = rt.nodes.get_mut(sub) {
                    sub_node.sources.remove(node_id);
                }
            }
        }
        // Strip these nodes from the pending queue if any were scheduled.
        rt.pending.retain(|n| !nodes.contains(n));
    });

    // Step 5: release every element handle the disposed owner created.
    // We do this AFTER recursing into children so that bottom-up
    // disposal order matches what the renderer expects (a child
    // element's release before its parent's is fine; the bridge
    // only complains if a parent is missing when a child reaches up).
    // Done with the runtime borrow released so a future renderer
    // that wants to call back into the reactive system (e.g. to
    // notify "element released") can do so.
    for handle in elements {
        crate::view::release_element(handle);
    }

    // Step 6: run cleanups in LIFO order, with no runtime borrow held
    // (cleanups may legitimately touch other parts of the runtime).
    for cleanup in cleanups.into_iter().rev() {
        cleanup();
    }
}

/// Register a callback to run when the current owner is disposed.
/// Calls accumulate in LIFO order, mirroring Solid / Leptos
/// `onCleanup` semantics.
///
/// No-op (with a warning in `debug`) if there is no current owner.
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    let registered = with_runtime(|rt| {
        let Some(owner_id) = rt.current_owner() else {
            return false;
        };
        if let Some(owner) = rt.owners.get_mut(owner_id) {
            owner.cleanups.push(Box::new(f));
            return true;
        }
        false
    });
    if !registered {
        debug_assert!(
            false,
            "on_cleanup called outside any owner — registration ignored"
        );
    }
}
