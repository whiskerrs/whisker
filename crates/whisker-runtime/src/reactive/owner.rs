//! Owner / scope API surface.
//!
//! [`Owner`] is the public-facing handle for a reactive scope —
//! the lifetime unit that ties together signals, effects, computed
//! values, view element handles, and cleanup callbacks. Disposing
//! an `Owner` cascades into its children, frees every node it
//! allocated, releases the element handles it owned, and runs its
//! cleanup callbacks in LIFO order.
//!
//! ## When to reach for these methods
//!
//! - Application code: **almost never**. `#[component]`,
//!   `provide_context`, `on_cleanup` etc. set up and tear down
//!   owners for you automatically.
//! - Framework extension code (custom control-flow primitives, a
//!   router, a custom list virtualizer): when you need to mount
//!   sub-trees whose lifetime is shorter than the surrounding
//!   component — that's where `Owner::new` / `owner.with` /
//!   `owner.dispose` come in.
//! - Tests: hand-driving owner lifecycle is convenient for
//!   reactive unit tests.
//!
//! See the crate-level docs for the conceptual model.
//!
//! The underlying [`Owner`] type is a `Copy` slotmap key
//! defined in [`super::runtime`]; the methods on this page are
//! attached to that type via an `impl` block.

use std::rc::Rc;

use super::runtime::{NodeId, Owner, Scope};
use super::with_runtime;

impl Owner {
    /// Create a new owner. If `parent` is `None` the current
    /// top-of-stack owner is used (or the owner becomes a root if
    /// the stack is empty).
    ///
    /// The new owner inherits its parent's `paused` flag — so a
    /// sub-component mounted while its containing route is
    /// suspended starts paused, and its effects won't fire until
    /// the route resumes.
    pub fn new(parent: Option<Owner>) -> Owner {
        with_runtime(|rt| {
            let parent = parent.or_else(|| rt.current_owner());
            let parent_paused = parent
                .and_then(|p| rt.owners.get(p))
                .map(|o| o.paused)
                .unwrap_or(false);
            let mut scope = Scope::new(parent);
            scope.paused = parent_paused;
            let id = rt.owners.insert(scope);
            if let Some(p) = parent {
                if let Some(parent_scope) = rt.owners.get_mut(p) {
                    parent_scope.children.push(id);
                }
            }
            id
        })
    }

    /// Create a parentless **root** owner, ignoring whatever owner is
    /// currently on the stack.
    ///
    /// Unlike [`Owner::new(None)`](Owner::new) — which adopts the
    /// current top-of-stack owner as parent — this always produces a
    /// detached root. Use it for **process-global singletons** whose
    /// lifetime must not be tied to the (possibly short-lived) owner
    /// that happens to be active when the singleton is first touched.
    ///
    /// The canonical case is a module that lazily mints an
    /// arena-backed handle on first access (e.g.
    /// `whisker-safe-area`): if that first access lands inside a
    /// per-route / per-component owner, minting under `new(None)` would
    /// free the handle when that scope disposes, and a later read would
    /// hit a disposed node. Minting under a `detached_root()` (then
    /// never disposing it) keeps the handle alive for the whole
    /// process — the intended semantics for a singleton.
    ///
    /// The returned owner is never auto-disposed; the caller is
    /// expected to leak it (i.e. drop the handle without calling
    /// [`dispose`](Owner::dispose)) for genuine process-lifetime data.
    pub fn detached_root() -> Owner {
        with_runtime(|rt| rt.owners.insert(Scope::new(None)))
    }

    /// Push `self` as the current scope, run `f`, pop back.
    /// Reactive primitives (`signal()`, `effect()`, `computed()`,
    /// view elements created via `render!`) allocated inside `f`
    /// will belong to this owner.
    pub fn with<R>(self, f: impl FnOnce() -> R) -> R {
        with_runtime(|rt| rt.owner_stack.push(self));
        let result = f();
        with_runtime(|rt| {
            let popped = rt.owner_stack.pop();
            debug_assert_eq!(
                popped,
                Some(self),
                "Owner::with: stack imbalance — owner pop didn't match push"
            );
        });
        result
    }

    /// Dispose `self`, freeing all its descendants, nodes, and
    /// running its cleanup callbacks.
    ///
    /// Recursive — disposes children first, then this owner. Safe
    /// to call even if the owner has already been disposed (no-op).
    pub fn dispose(self) {
        // Everything is pulled out under a short borrow rather than
        // held across the recursion — deeper levels re-enter the
        // runtime.
        let children;
        let nodes;
        let cleanups;
        let parent;
        let mount_fn;
        let elements;
        {
            let removed = with_runtime(|rt| rt.owners.remove(self));
            let Some(o) = removed else { return };
            children = o.children;
            nodes = o.nodes;
            cleanups = o.cleanups;
            parent = o.parent;
            mount_fn = o.mount_fn;
            elements = o.elements;
        }

        // A component owner must leave the hot-reload registry, or
        // `owners_for_fn` hands remount logic a dangling Owner.
        if let Some(fp) = mount_fn {
            with_runtime(|rt| {
                if let Some(list) = rt.component_owners.get_mut(&fp) {
                    list.retain(|o| *o != self);
                    if list.is_empty() {
                        rt.component_owners.remove(&fp);
                    }
                }
            });

            // Likewise its remountable MountSites: an orphan site
            // survives cascading disposal and the next
            // `remount_components_for` then operates on freed parent /
            // body_root handles.
            //
            // `site.owner` is `None` *during* a remount (the
            // take-then-reinstall window in `remount_one`), so this
            // scan can't evict a site that is mid-flight.
            with_runtime(|rt| {
                let stale: Vec<super::component::MountId> = rt
                    .mount_sites
                    .iter()
                    .filter_map(|(id, site)| {
                        if site.owner == Some(self) {
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

        if let Some(p) = parent {
            with_runtime(|rt| {
                if let Some(parent_scope) = rt.owners.get_mut(p) {
                    parent_scope.children.retain(|&c| c != self);
                }
            });
        }

        for child in children {
            child.dispose();
        }

        // Cleanups run before this owner's nodes are freed, so an
        // `on_cleanup` can still read and write the signals it closed
        // over (what Solid / Leptos / React all allow). Children are
        // already disposed above, so a *child's* signal still reads as
        // gone. LIFO, and with no runtime borrow held — a cleanup may
        // re-enter the runtime.
        for cleanup in cleanups.into_iter().rev() {
            cleanup();
        }

        // Freeing a node also detaches it from every subscriber list
        // it was on, so no live node notifies a freed slot later.
        // Arc-signal back-refs are collected here and unsubscribed
        // below, outside the borrow — those callees re-enter the
        // runtime.
        let arc_unsubscribes: Vec<(Rc<dyn super::runtime::ArcSubscription>, NodeId)> =
            with_runtime(|rt| {
                let mut out: Vec<(Rc<dyn super::runtime::ArcSubscription>, NodeId)> = Vec::new();
                for node_id in &nodes {
                    let Some(node) = rt.nodes.remove(*node_id) else {
                        continue;
                    };
                    for source in node.sources {
                        if let Some(src_node) = rt.nodes.get_mut(source) {
                            src_node.subscribers.remove(node_id);
                        }
                    }
                    // A signal this owner held may have been read by an
                    // outer effect, so clear the reverse edge too.
                    for sub in node.subscribers {
                        if let Some(sub_node) = rt.nodes.get_mut(sub) {
                            sub_node.sources.remove(node_id);
                        }
                    }
                    for arc_src in node.arc_sources {
                        out.push((arc_src, *node_id));
                    }
                }
                // A scheduled node left in these queues would be re-run
                // from its freed slot on the next flush / resume.
                rt.pending.retain(|n| !nodes.contains(n));
                rt.deferred.retain(|n| !nodes.contains(n));
                out
            });

        // An Arc-backed signal outlives its subscribers, so pruning
        // here is what keeps its list from accumulating dead
        // `NodeId`s across transient subscribers.
        for (arc_src, subscriber) in arc_unsubscribes {
            arc_src.unsubscribe(subscriber);
        }

        // Element release comes after the child recursion so the
        // renderer sees children released before their parents, and
        // runs with no runtime borrow held so a renderer may call back
        // into the reactive system.
        for handle in elements {
            crate::view::release_element(handle);
        }
    }

    /// Pause `self` (and its descendants): effects and computeds
    /// whose scope is the paused subtree skip flush. Their
    /// scheduled re-runs land on the runtime's `deferred` list
    /// until [`Owner::resume`] drains them back.
    ///
    /// Idempotent — pausing an already-paused owner is a no-op.
    /// The cascade walks the children tree breadth-first; new
    /// descendants created while paused inherit the flag via
    /// [`Owner::new`].
    ///
    /// Used by `StackLayout` to freeze back-stack entries that are
    /// mounted-but-off-screen, matching iOS
    /// `UINavigationController` / Android Fragment back-stack
    /// semantics: state survives but no CPU is spent on
    /// signal-driven re-renders behind the top route.
    pub fn pause(self) {
        with_runtime(|rt| {
            let mut stack = vec![self];
            while let Some(id) = stack.pop() {
                let Some(o) = rt.owners.get_mut(id) else {
                    continue;
                };
                if o.paused {
                    continue;
                }
                o.paused = true;
                stack.extend(o.children.iter().copied());
            }
        });
    }

    /// Resume `self` (and its descendants): clear the paused flag
    /// and move any of its deferred effects back onto the pending
    /// queue so they fire on the next flush.
    ///
    /// Idempotent. Iterates [`super::runtime::ReactiveRuntime::deferred`]
    /// and re-queues every node whose owner is no longer paused —
    /// including descendants resumed by this cascade, and any
    /// deferred node whose owner happens to have been unpaused by
    /// an earlier call.
    pub fn resume(self) {
        let any_resumed = with_runtime(|rt| {
            let mut stack = vec![self];
            let mut any = false;
            while let Some(id) = stack.pop() {
                let Some(o) = rt.owners.get_mut(id) else {
                    continue;
                };
                if !o.paused {
                    continue;
                }
                o.paused = false;
                any = true;
                stack.extend(o.children.iter().copied());
            }
            if !any {
                return false;
            }
            // Nodes disposed while their owner was paused are still
            // listed here; they get dropped rather than re-queued.
            let deferred = std::mem::take(&mut rt.deferred);
            for node in deferred {
                let still_paused = rt
                    .nodes
                    .get(node)
                    .and_then(|n| rt.owners.get(n.owner))
                    .map(|o| o.paused);
                match still_paused {
                    Some(false) => {
                        if !rt.pending.contains(&node) {
                            rt.pending.push(node);
                        }
                    }
                    Some(true) => rt.deferred.push(node),
                    None => {} // node or owner is gone; drop silently
                }
            }
            true
        });
        if any_resumed {
            crate::host_wake::wake_runtime();
        }
    }

    /// Whether `self` is currently paused. Mainly for tests;
    /// production code should drive pause / resume from the
    /// lifecycle layer and not branch on the flag directly.
    pub fn is_paused(self) -> bool {
        with_runtime(|rt| rt.owners.get(self).map(|o| o.paused).unwrap_or(false))
    }
}

/// Register a callback to run when the current owner is disposed.
/// Calls accumulate in LIFO order, mirroring Solid / Leptos
/// `onCleanup` semantics.
///
/// No-op (with a warning in `debug`) if there is no current owner.
///
/// Kept as a free function (not a method on [`Owner`]) because it
/// operates on whatever owner happens to be at the top of the
/// runtime's owner stack — the caller can't sensibly name it.
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    let registered = with_runtime(|rt| {
        let Some(owner_id) = rt.current_owner() else {
            return false;
        };
        if let Some(scope) = rt.owners.get_mut(owner_id) {
            scope.cleanups.push(Box::new(f));
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
