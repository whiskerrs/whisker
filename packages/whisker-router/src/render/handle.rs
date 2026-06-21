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

use crate::core::{
    CompiledTree, NavError, Navigator, NodePath, RouteInstance, RouteState, Scope, Target,
};
use crate::render::registry::{RenderFn, RouteRegistry, Transition};

/// A repointable pose binding for one stack wrapper: the controller whose
/// progress drives it + the role it plays. The swipe-back gesture sets
/// both wrappers' bindings to the **same** controller (Top / Under) so one
/// scrubbed progress moves the pair — the identical coordinated model the
/// non-interactive push/pop uses.
#[derive(Copy, Clone)]
pub struct PoseBinding {
    /// The controller this wrapper's pose currently reads.
    pub ctrl: RwSignal<AnimationController>,
    /// The role (`Top` / `Under`) this wrapper plays.
    pub role: RwSignal<crate::render::transition::Role>,
}

/// A live bridge a rendered [`Stack`](crate::render::Stack) registers so
/// the iOS swipe-back gesture can drive its top/under wrappers as a
/// coordinated pair. Re-published on every reconcile so it always points
/// at the current top.
#[derive(Clone)]
pub struct StackBridge {
    /// The top wrapper's transition controller (what the gesture scrubs).
    pub top_ctrl: Option<AnimationController>,
    /// The top wrapper's pose binding.
    pub top_pose: Option<PoseBinding>,
    /// The revealed-under wrapper's pose binding, if any.
    pub under_pose: Option<PoseBinding>,
    /// The transition kind of the top entry.
    pub transition: Transition,
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
    pub fn new(tree: CompiledTree, registry: RouteRegistry) -> Self {
        let owner = Owner::detached_root();
        let initial = RouteState::initial(&tree);
        let state = owner.with(|| RwSignal::new(initial));
        RouterHandle {
            inner: Rc::new(Inner {
                tree,
                registry,
                state,
                bridges: RefCell::new(HashMap::new()),
                _owner: owner,
            }),
        }
    }

    /// Register / refresh the gesture bridge for the stack at `path`.
    /// Called by [`Stack`](crate::render::Stack) reconcile each time its
    /// top wrapper changes.
    pub fn set_stack_bridge(&self, path: NodePath, bridge: StackBridge) {
        self.inner.bridges.borrow_mut().insert(path, bridge);
    }

    /// (Test only) the bridge registered for the stack at `path`,
    /// regardless of whether it can currently pop — used to assert a
    /// survivor's resting pose after a coordinated pop.
    #[doc(hidden)]
    pub fn active_stack_bridge_for_test(&self, path: &NodePath) -> Option<StackBridge> {
        self.inner.bridges.borrow().get(path).cloned()
    }

    /// The gesture bridge for the **deepest active stack** — the one a
    /// swipe-back would pop. Walks the active chain to the bottom-most
    /// registered stack that currently has something to pop.
    pub fn active_stack_bridge(&self) -> Option<StackBridge> {
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
    pub fn transition(&self, id: &str) -> Transition {
        self.inner.registry.transition(id)
    }

    /// The raw state signal. Prefer [`slice_at`](Self::slice_at) /
    /// [`current`](Self::current) for fine-grained reads.
    pub fn state(&self) -> RwSignal<RouteState> {
        self.inner.state
    }

    /// A `computed` of the **current (shown) leaf instance** — derived
    /// by walking the state, never stored.
    pub fn current(&self) -> ReadSignal<RouteInstance> {
        let state = self.inner.state;
        computed(move || state.with(|s| s.current().clone()))
    }

    /// A `computed` of the [`RouteState`] subtree at `path`.
    ///
    /// Returns the subtree (or `None` if `path` does not address a live
    /// node in the current state — e.g. a not-yet-entered container).
    /// Memoised by `PartialEq`, so subscribers only re-run when *this*
    /// subtree changes.
    pub fn slice_at(&self, path: NodePath) -> ReadSignal<Option<RouteState>> {
        let state = self.inner.state;
        computed(move || state.with(|s| state_at(s, &path).cloned()))
    }

    /// A `computed` of the selected branch index of the `Switch` at
    /// `path` (or `None` if that node is not a live `Switch`). The
    /// reactive primitive behind [`use_active_tab`](crate::render::use_active_tab).
    pub fn selected_at(&self, path: NodePath) -> ReadSignal<Option<usize>> {
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

    /// Navigate to `target` (relative resolution, no params).
    pub fn navigate(&self, target: &Target) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.navigate(target))
    }

    /// Navigate to `target`, attaching `instance`'s params.
    pub fn navigate_with(&self, target: &Target, instance: RouteInstance) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.navigate_with(target, instance))
    }

    /// Navigate to `target` within an explicit [`Scope`].
    pub fn navigate_within(&self, target: &Target, scope: &Scope) -> Result<(), NavError> {
        self.with_navigator(|nav| nav.navigate_within(target, scope))
    }

    /// Select the `Switch` branch toward `target` (the tab-switch
    /// primitive). Returns the resolved [`NodePath`].
    pub fn select(&self, target: &Target) -> Result<NodePath, NavError> {
        self.with_navigator(|nav| nav.select(target))
    }

    /// Pop the deepest non-trivial stack. Returns `true` if something
    /// was popped.
    pub fn back(&self) -> bool {
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
