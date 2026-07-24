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
        // Step 1: collect what needs cleaning. We pull data out of
        // the runtime in a short borrow rather than holding it
        // through the recursion, because each level may itself need
        // to mutate the runtime (running cleanup callbacks does not,
        // but symmetrically we keep the pattern simple by avoiding
        // nested borrows).
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

        // Step 1b: if this was a component owner, scrub the hot-
        // reload registry so the fn pointer doesn't list a freed
        // slot. Without this, A6's `owners_for_fn` would return a
        // dangling Owner and remount logic would fault.
        if let Some(fp) = mount_fn {
            with_runtime(|rt| {
                if let Some(list) = rt.component_owners.get_mut(&fp) {
                    list.retain(|o| *o != self);
                    if list.is_empty() {
                        rt.component_owners.remove(&fp);
                    }
                }
            });

            // Step 1c: also clean up any remountable MountSite whose
            // owner is this one. Without this scrub, cascading
            // disposal (e.g. parent component re-mounts and discards
            // its sub-tree) leaves orphan MountSites behind, and
            // the next `remount_components_for` call processes
            // them — operating on freed parent / body_root handles,
            // with visible corruption (issue #17 follow-up).
            //
            // `site.owner` is `None` *during* a remount (the
            // takes-then-reinstalls window in `remount_one`), so
            // this scan won't accidentally evict the site that's
            // mid-flight. It only matches MountSites whose
            // component owner is the one actually being disposed.
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

        // Step 2: detach from parent's children list.
        if let Some(p) = parent {
            with_runtime(|rt| {
                if let Some(parent_scope) = rt.owners.get_mut(p) {
                    parent_scope.children.retain(|&c| c != self);
                }
            });
        }

        // Step 3: dispose descendants (post-order — bottom up).
        for child in children {
            child.dispose();
        }

        // Step 4: run THIS owner's cleanups BEFORE freeing its nodes, so an
        // `on_cleanup` callback can still read/write the signals it closed
        // over — the natural expectation (Solid / Leptos / React all allow
        // it) and consistent with `set` on a freed signal already being a
        // silent no-op (`try_write_and_notify`). Freeing first (the old
        // order) made a same-owner `get` in a cleanup panic with `signal
        // disposed`; worse, when the cleanup ran mid-`tick_frame` (a router
        // screen disposed on a transition's finish), that panic unwound the
        // whole frame and stranded unrelated reactive work (frozen
        // animations, dead event handlers). Children are already disposed
        // above, so a cleanup reading a *child's* signal still gets nothing —
        // but that's the uncommon case; own-signal reads are what callers
        // expect. LIFO order, no runtime borrow held (cleanups may touch the
        // runtime).
        for cleanup in cleanups.into_iter().rev() {
            cleanup();
        }

        // Step 5: free every node this owner allocated. For effects
        // / computed values, also detach them from any subscriber
        // list they were on, so other live nodes don't try to
        // notify a freed slot later.
        //
        // Arc-signal back-references (`arc_sources`) get collected
        // here and unsubscribed below, outside the runtime borrow —
        // the unsubscribe callees may re-enter the runtime.
        let arc_unsubscribes: Vec<(Rc<dyn super::runtime::ArcSubscription>, NodeId)> =
            with_runtime(|rt| {
                let mut out: Vec<(Rc<dyn super::runtime::ArcSubscription>, NodeId)> = Vec::new();
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
                    // Collect arc-signal back-refs so we can call
                    // `unsubscribe` outside the runtime borrow.
                    for arc_src in node.arc_sources {
                        out.push((arc_src, *node_id));
                    }
                }
                // Strip these nodes from the pending and deferred queues
                // if any were scheduled — otherwise a later flush /
                // resume would try to re-run a freed slot.
                rt.pending.retain(|n| !nodes.contains(n));
                rt.deferred.retain(|n| !nodes.contains(n));
                out
            });

        // Tell every Arc-backed signal that one of our disposed
        // nodes used to be on its subscriber list. The signal itself
        // stays alive (Arc refcount), but pruning here keeps its
        // list bounded so a long-lived signal doesn't accumulate
        // dead `NodeId`s from every transient subscriber that came
        // and went.
        for (arc_src, subscriber) in arc_unsubscribes {
            arc_src.unsubscribe(subscriber);
        }

        // Step 6: release every element handle the disposed owner
        // created. We do this AFTER recursing into children so that
        // bottom-up disposal order matches what the renderer
        // expects (a child element's release before its parent's
        // is fine; the bridge only complains if a parent is missing
        // when a child reaches up). Done with the runtime borrow
        // released so a future renderer that wants to call back
        // into the reactive system (e.g. to notify "element
        // released") can do so.
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
            // Drain deferred → pending for every node whose owner
            // is no longer paused. Stale entries (node disposed
            // under a paused owner) are dropped here.
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
