//! [`RouterHandle`] — the signal-backed navigation handle, plus
//! [`use_navigator`] / [`provide_router`] context plumbing.
//!
//! Phase-1's [`Navigator`](crate::core::Navigator) borrows
//! `&mut RouteState`; that is fine for pure-logic tests but not for the
//! UI, where many components read the state reactively and a handful of
//! event handlers mutate it. [`RouterHandle`] is the reactive wrapper:
//! it owns the immutable [`CompiledTree`], the [`RouteRegistry`], and a
//! single `RwSignal<RouteState>`. The five navigation verbs
//! ([`navigate`](RouterHandle::navigate), [`select`](RouterHandle::select),
//! [`back`](RouterHandle::back), [`replace`](RouterHandle::replace),
//! [`pop_to`](RouterHandle::pop_to), [`reset`](RouterHandle::reset)) each
//! **clone the state, run the Phase-1 `Navigator` op, and write the
//! signal back** — so the entire navigation domain stays in `core` and
//! the handle is a thin reactive shell.
//!
//! ## `current` is derived, never stored
//!
//! [`current`](RouterHandle::current) is a `computed` over the state
//! signal — there is no separate "current screen" field, matching the
//! design doc's "derived `current`" rule.
//!
//! ## Fine-grained reads
//!
//! [`slice_at`](RouterHandle::slice_at) returns a `computed` that
//! extracts the [`RouteState`] subtree at a [`NodePath`]. Because
//! `computed` only notifies subscribers when its value *changes*
//! (`RouteState: PartialEq`), a push into tab A's stack produces an
//! **unchanged** slice for tab B's `Outlet`, so tab B does not
//! re-render. That is what keeps navigation fine-grained even though all
//! state lives in one signal — see `crate::render` for the honest
//! limits.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use whisker::runtime::reactive::Owner;
use whisker::{AnimationController, ReadSignal, RwSignal, computed, provide_context, use_context};

use crate::core::{CompiledTree, NavError, Navigator, NodePath, RouteInstance, RouteState, Target};
use crate::render::registry::{LayoutFn, LayoutRegistry, RenderFn, RouteRegistry, RouteSet};
use crate::render::transition::RouteTransition;

/// What [`RouterHandle::navigate`] accepts: a **URL** string (matched
/// against the route patterns, binding its `:param`s) or an explicit
/// [`Target`]. Both lower to a `(Target, RouteInstance)` the navigator runs.
pub trait IntoNav {
    /// Resolve `self` against `tree` into a target + its bound params.
    fn into_nav(self, tree: &CompiledTree) -> Result<(Target, RouteInstance), NavError>;
}

impl IntoNav for &str {
    fn into_nav(self, tree: &CompiledTree) -> Result<(Target, RouteInstance), NavError> {
        let (route_id, params) = tree.match_url(self).ok_or(NavError::NoSuchTarget)?;
        let mut instance = RouteInstance::new(NodePath::root());
        instance.params = params;
        Ok((Target::id(route_id), instance))
    }
}

impl IntoNav for &Target {
    fn into_nav(self, _tree: &CompiledTree) -> Result<(Target, RouteInstance), NavError> {
        Ok((self.clone(), RouteInstance::new(NodePath::root())))
    }
}

/// A repointable pose binding for one stack wrapper: the controller whose
/// progress drives it + the role it plays. The swipe-back gesture sets
/// both wrappers' bindings to the **same** controller (Top / Under) so one
/// scrubbed progress moves the pair — the identical coordinated model the
/// non-interactive push/pop uses.
#[derive(Copy, Clone)]
pub(crate) struct PoseBinding {
    /// The controller this wrapper's pose currently reads.
    pub ctrl: RwSignal<AnimationController>,
    /// The role (`Top` / `Under`) this wrapper plays.
    pub role: RwSignal<crate::render::transition::Role>,
    /// The pose mode (normal transition vs predictive-back preview). A
    /// back gesture flips this to `Predictive(edge)` for the drag, then
    /// the settle/cancel restores the route transition.
    pub mode: RwSignal<crate::render::transition::PoseMode>,
}

/// A live bridge a rendered [`Stack`](crate::render::Stack) registers so
/// the iOS swipe-back / Android predictive-back gestures can drive its
/// top/under wrappers as a coordinated pair. Re-published on every
/// reconcile so it always points at the current top.
#[derive(Clone)]
pub(crate) struct StackBridge {
    /// The top wrapper's transition controller (what the gesture scrubs).
    pub top_ctrl: Option<AnimationController>,
    /// The top wrapper's pose binding.
    pub top_pose: Option<PoseBinding>,
    /// The revealed-under wrapper's pose binding, if any.
    pub under_pose: Option<PoseBinding>,
    /// The stack's backdrop-dim **drive**: when `Some(ctrl)`, the dim
    /// opacity reactively follows `(1 - ctrl.value()) * PB_MAX_DIM`, so it
    /// darkens during the drag AND animates in lockstep with the settle
    /// run. A back gesture sets this to the top controller on `begin` and
    /// clears it (`None` → dim 0) when the settle finishes.
    pub dim_drive: Option<RwSignal<Option<AnimationController>>>,
    /// Whether this stack currently has something to pop.
    pub can_back: bool,
}

/// Shared, signal-backed navigation handle.
///
/// Cloning shares the underlying tree / registry / state signal (it is
/// `Rc`-backed), so the handle can be passed freely into closures and
/// child components without wrapping. Publish it once with
/// [`provide_router`] and retrieve it anywhere below with
/// [`use_navigator`].
#[derive(Clone)]
pub struct RouterHandle {
    inner: Rc<Inner>,
}

struct Inner {
    tree: CompiledTree,
    registry: RouteRegistry,
    /// `Layout(X)` chrome per container path (from `routes!`); applied by
    /// `mount_node`.
    layouts: LayoutRegistry,
    state: RwSignal<RouteState>,
    /// Per-stack gesture bridges, keyed by the stack's [`NodePath`].
    /// Registered by [`Stack`](crate::render::Stack) reconcile; read by
    /// the swipe-back gesture to find the deepest active stack's top
    /// wrapper + controller.
    bridges: RefCell<HashMap<NodePath, StackBridge>>,
    /// Owns the `state` signal. A detached root so the signal lives for
    /// the handle's lifetime, not the (often transient) owner that
    /// happened to be current at construction — the same footgun the
    /// old `RouteStack` guards against. See `Owner::detached_root`.
    _owner: Owner,
}

impl RouterHandle {
    /// Build a handle from a [`CompiledTree`] and its [`RouteRegistry`],
    /// seeding the state with [`RouteState::initial`].
    pub fn new(routes: impl Into<RouteSet>) -> Self {
        let RouteSet {
            tree,
            registry,
            layouts,
        } = routes.into();
        let owner = Owner::detached_root();
        let initial = RouteState::initial(&tree);
        let state = owner.with(|| RwSignal::new(initial));
        RouterHandle {
            inner: Rc::new(Inner {
                tree,
                registry,
                layouts,
                state,
                bridges: RefCell::new(HashMap::new()),
                _owner: owner,
            }),
        }
    }

    /// The `Layout(X)` chrome registered for the container at `path`, if any.
    pub(crate) fn layout_at(&self, path: &NodePath) -> Option<LayoutFn> {
        self.inner.layouts.get(path).cloned()
    }

    /// Register / refresh the gesture bridge for the stack at `path`.
    /// Called by [`Stack`](crate::render::Stack) reconcile each time its
    /// top wrapper changes.
    pub(crate) fn set_stack_bridge(&self, path: NodePath, bridge: StackBridge) {
        self.inner.bridges.borrow_mut().insert(path, bridge);
    }

    /// (Test only) the bridge registered for the stack at `path`,
    /// regardless of whether it can currently pop — used to assert a
    /// survivor's resting pose after a coordinated pop.
    #[cfg(test)]
    pub(crate) fn active_stack_bridge_for_test(&self, path: &NodePath) -> Option<StackBridge> {
        self.inner.bridges.borrow().get(path).cloned()
    }

    /// The gesture bridge for the **deepest active stack** — the one a
    /// swipe-back would pop. Walks the active chain to the bottom-most
    /// registered stack that currently has something to pop.
    pub(crate) fn active_stack_bridge(&self) -> Option<StackBridge> {
        let state = self.inner.state.get_untracked();
        let bridges = self.inner.bridges.borrow();
        let mut found: Option<StackBridge> = None;
        for node in state.active_chain() {
            if let RouteState::Stack(s) = node {
                if let Some(b) = bridges.get(&s.path) {
                    if b.can_back {
                        found = Some(b.clone());
                    }
                }
            }
        }
        found
    }

    /// The compiled route tree (immutable).
    pub fn tree(&self) -> &CompiledTree {
        &self.inner.tree
    }

    /// The route registry.
    pub fn registry(&self) -> &RouteRegistry {
        &self.inner.registry
    }

    /// The render closure for a route id, if registered.
    pub fn render_fn(&self, id: &str) -> Option<RenderFn> {
        self.inner.registry.render_fn(id)
    }

    /// The transition configured for a route id.
    pub fn transition(&self, id: &str) -> RouteTransition {
        self.inner.registry.transition(id)
    }

    /// The raw state signal (test only). Production code reads via
    /// [`slice_at`](Self::slice_at) / [`current`](Self::current) and mutates
    /// via the verbs.
    #[cfg(test)]
    pub(crate) fn state(&self) -> RwSignal<RouteState> {
        self.inner.state
    }

    /// A `computed` of the **current (shown) leaf instance** — derived
    /// by walking the state, never stored.
    pub(crate) fn current(&self) -> ReadSignal<RouteInstance> {
        let state = self.inner.state;
        computed(move || state.with(|s| s.current().clone()))
    }

    /// A `computed` of the [`RouteState`] subtree at `path`.
    ///
    /// Returns the subtree (or `None` if `path` does not address a live
    /// node in the current state — e.g. a not-yet-entered container).
    /// Memoised by `PartialEq`, so subscribers only re-run when *this*
    /// subtree changes.
    pub(crate) fn slice_at(&self, path: NodePath) -> ReadSignal<Option<RouteState>> {
        let state = self.inner.state;
        computed(move || state.with(|s| state_at(s, &path).cloned()))
    }

    /// A `computed` of the selected branch index of the `Switch` at
    /// `path` (or `None` if that node is not a live `Switch`). The
    /// reactive primitive for reading tab selection (the built-in
    /// [`TabBar`](crate::render::TabBar) derives its active tab this way).
    pub(crate) fn selected_at(&self, path: NodePath) -> ReadSignal<Option<usize>> {
        let state = self.inner.state;
        computed(move || {
            state.with(|s| match state_at(s, &path) {
                Some(RouteState::Switch(sw)) => Some(sw.selected),
                _ => None,
            })
        })
    }

    // ----- the five verbs (clone → core op → write back) ------------

    /// Mutate the state via a Phase-1 [`Navigator`] op, writing the
    /// result back into the signal. The closure receives a `Navigator`
    /// bound to a *clone* of the current state; whatever it returns is
    /// propagated to the caller.
    fn with_navigator<T>(&self, op: impl FnOnce(&mut Navigator) -> T) -> T {
        let mut state = self.inner.state.get();
        let out = {
            let mut nav = Navigator::new(&self.inner.tree, &mut state);
            op(&mut nav)
        };
        self.inner.state.set(state);
        out
    }

    /// Navigate forward. Accepts either a **URL** string —
    /// `navigate("/detail/123")`, matched against the route patterns
    /// (`/detail/:id`) with its `:param`s bound — or a [`Target`] —
    /// `navigate(&Target::id("detail"))`. A shared route resolves to the
    /// right branch via the usual relative resolution. Returns
    /// [`NavError::NoSuchTarget`] when nothing matches.
    pub fn navigate(&self, to: impl IntoNav) -> Result<(), NavError> {
        let (target, instance) = to.into_nav(&self.inner.tree)?;
        self.with_navigator(|nav| nav.navigate_with(&target, instance))
    }

    /// Select the `Switch` branch toward `target` (the tab-switch
    /// primitive), preserving the target tab's retained history.
    pub fn select(&self, target: &Target) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.select(target))
    }

    /// Pop the deepest non-trivial stack. [`NavError::NothingToPop`] when
    /// there is nothing to pop (e.g. at a tab root).
    pub fn back(&self) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.back())
    }

    /// Swap the top of the current stack with `target` (same stack only).
    pub fn replace(&self, target: &Target) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.replace(target))
    }

    /// Pop the current stack until `target` is the top (same stack only).
    pub fn pop_to(&self, target: &Target) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.pop_to(target))
    }

    /// Replace the entire current stack with `[target]`.
    pub fn reset(&self, target: &Target) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.reset(target))
    }
}

/// Walk `state` to the subtree at `path` (positional descent mirroring
/// the active/branch structure). Returns `None` if any index along the
/// way is out of range for the live state (e.g. a container that has not
/// been entered yet).
pub(crate) fn state_at<'a>(state: &'a RouteState, path: &NodePath) -> Option<&'a RouteState> {
    let mut node = state;
    for &idx in &path.0 {
        node = match node {
            // A Switch's children are its branches, indexed directly.
            RouteState::Switch(s) => s.branches.get(idx)?,
            // A Stack's children are reached through whichever history
            // entry instantiated child index `idx`. The active subtree
            // for that child is the *top-most* entry whose `child` path
            // ends in `idx`.
            RouteState::Stack(s) => {
                let entry = s
                    .history
                    .iter()
                    .rev()
                    .find(|e| e.child.0.last() == Some(&idx))?;
                &entry.state
            }
            RouteState::Route(_) => return None,
        };
    }
    Some(node)
}

/// Publish a [`RouterHandle`] into context so descendants can reach it
/// via [`use_navigator`].
pub fn provide_router(handle: RouterHandle) {
    provide_context(handle);
}

/// Retrieve the [`RouterHandle`] from context.
///
/// # Panics
///
/// Panics if no [`RouterHandle`] was published above the caller (via
/// [`provide_router`] or the [`Router`](crate::render::Router)
/// component). A silent `None` would hide the misuse.
pub fn use_navigator() -> RouterHandle {
    use_context::<RouterHandle>()
        .expect("use_navigator() called outside a Router / provide_router ancestor")
}

/// Context marking the route leaf a routed component is mounted under, so
/// the `use_param` / `use_params` hooks can find **its** params. Published
/// by `mount_route` just before it renders the route's component.
#[derive(Clone)]
pub(crate) struct RouteScope(pub NodePath);

/// Reactively read a named path parameter of the route the calling
/// component is mounted under (`Route("detail/:id", …)` →
/// `use_param("id")`). Returns `None` while the param is absent.
///
/// A signal-framework "hook": it reads the router context and returns a
/// derived signal — the whisker analogue of leptos's `use_params` /
/// SolidJS's `useParams`. Call it from a routed `#[component]`'s body and
/// `.get()` the result wherever the value is needed; it re-derives when the
/// route's params change.
///
/// # Panics
///
/// Panics if called outside a routed screen (no [`RouteScope`] in context).
pub fn use_param(name: &str) -> ReadSignal<Option<String>> {
    let handle = use_navigator();
    let scope = use_context::<RouteScope>()
        .expect("use_param() called outside a routed screen (no Route ancestor)");
    let slice = handle.slice_at(scope.0);
    let name = name.to_string();
    computed(move || match slice.get() {
        Some(RouteState::Route(inst)) => inst.params.get(&name).cloned(),
        _ => None,
    })
}

/// The current location as a URL string (e.g. `/podcast/42`) — the
/// reactive "where am I" read, analogous to Expo Router's `usePathname()`.
///
/// Derived from the active leaf's path; recomputes whenever navigation
/// changes it. This is the general primitive custom chrome uses to reflect
/// the current route (the built-in [`TabBar`](crate::render::TabBar) does
/// this matching itself, so a tab bar needs no hook). Returns `"/"` if the
/// path can't be resolved (should not happen for a mounted router).
pub fn use_pathname() -> ReadSignal<String> {
    let handle = use_navigator();
    let current = handle.current();
    computed(move || {
        let path = current.get().path;
        handle
            .tree()
            .url_of(&path)
            .unwrap_or_else(|| "/".to_string())
    })
}
