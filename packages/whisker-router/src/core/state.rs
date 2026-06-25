//! The dynamic [`RouteState`] — the [`CompiledTree`] *instantiated*
//! with runtime navigation state.
//!
//! The state tree mirrors the static tree's shape, carrying exactly two
//! kinds of mutable state:
//!
//! - [`StackState`] (for a `Stack` node): a `history` of [`StackEntry`]s.
//!   Each entry is a child *instance* — a `Route` instance carries its
//!   concrete param values; a container entry carries the nested
//!   [`RouteState`] of that whole subtree.
//! - [`SwitchState`] (for a `Switch` node): a `selected` branch index,
//!   plus the (lazily built) nested state of every branch so each branch
//!   keeps its own history while buried.
//!
//! There is **no stored `current`**. [`RouteState::current`] derives the
//! active leaf every time by walking `history.top` / `selected`.

use std::collections::BTreeMap;

use super::tree::{CompiledTree, NodePath, RouteTree, SwitchDef};

/// The runtime state of one node, mirroring the static tree's shape.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteState {
    /// A `Route` node instance. If the static `Route` has children, the
    /// child states are in `children` (mirroring the static tree).
    Route(RouteInstance),
    /// An ordered container's history.
    Stack(StackState),
    /// A parallel container's selection + per-branch nested state.
    Switch(SwitchState),
}

/// A concrete `Route` instance: which static node it is + its param
/// values.
///
/// `post(1)` and `post(2)` are two distinct instances of the same
/// static `Route` node, differing only in [`params`](RouteInstance::params).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RouteInstance {
    /// The static node this instantiates.
    pub path: NodePath,
    /// Concrete param values, keyed by segment name (e.g. `id` → `42`).
    pub params: BTreeMap<String, String>,
    /// Child states, mirroring the static Route's children. Empty for
    /// leaf Routes; non-empty when the Route has Stack/Switch/Route children.
    pub children: Vec<RouteState>,
}

impl RouteInstance {
    /// A bare route instance with no params and no children.
    pub fn new(path: NodePath) -> Self {
        RouteInstance {
            path,
            params: BTreeMap::new(),
            children: Vec::new(),
        }
    }

    /// A route instance with one param.
    pub fn with_param(path: NodePath, key: impl Into<String>, value: impl Into<String>) -> Self {
        let mut params = BTreeMap::new();
        params.insert(key.into(), value.into());
        RouteInstance {
            path,
            params,
            children: Vec::new(),
        }
    }
}

/// One entry in a [`StackState`]'s history: a child instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackEntry {
    /// The static child this entry instantiates (a child index of the
    /// owning `Stack`, expressed as a full [`NodePath`]).
    pub child: NodePath,
    /// The nested runtime state of that child subtree.
    pub state: RouteState,
}

/// A `Stack` node's runtime state: its non-empty history.
///
/// The **top** of `history` is the active child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackState {
    /// The static node this instantiates.
    pub path: NodePath,
    /// The entered children, oldest first; the last is the active one.
    /// Invariant: never empty after [`RouteState::initial`].
    pub history: Vec<StackEntry>,
}

/// A `Switch` node's runtime state: which branch is selected + every
/// branch's nested state.
///
/// All branches are kept alive (the parallel-container property), so a
/// buried tab retains its own history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchState {
    /// The static node this instantiates.
    pub path: NodePath,
    /// The selected branch index.
    pub selected: usize,
    /// Nested state for every branch, by branch index.
    pub branches: Vec<RouteState>,
}

impl RouteState {
    /// Build the **initial** state for a whole [`CompiledTree`]: every
    /// `Stack` seeded with its first child, every `Switch` set to its
    /// declared default branch (else `0`), recursively.
    pub fn initial(tree: &CompiledTree) -> RouteState {
        Self::initial_at(tree, &NodePath::root())
    }

    /// Build the initial state for the subtree rooted at `path`.
    pub fn initial_at(tree: &CompiledTree, path: &NodePath) -> RouteState {
        let node = tree
            .node_at(path)
            .expect("initial_at: path must address a node in the tree");
        match node {
            RouteTree::Route(_, children) => {
                let child_states: Vec<RouteState> = children
                    .iter()
                    .enumerate()
                    .map(|(i, _)| Self::initial_at(tree, &path.child(i)))
                    .collect();
                RouteState::Route(RouteInstance {
                    path: path.clone(),
                    params: BTreeMap::new(),
                    children: child_states,
                })
            }
            RouteTree::Stack(children) => {
                assert!(!children.is_empty(), "a Stack must have at least one child");
                let child_path = path.child(0);
                RouteState::Stack(StackState {
                    path: path.clone(),
                    history: vec![StackEntry {
                        child: child_path.clone(),
                        state: Self::initial_at(tree, &child_path),
                    }],
                })
            }
            RouteTree::Switch(def, branches) => {
                assert!(
                    !branches.is_empty(),
                    "a Switch must have at least one branch"
                );
                let selected = clamp_default(def, branches.len());
                let branch_states = (0..branches.len())
                    .map(|i| Self::initial_at(tree, &path.child(i)))
                    .collect();
                RouteState::Switch(SwitchState {
                    path: path.clone(),
                    selected,
                    branches: branch_states,
                })
            }
        }
    }

    /// The [`NodePath`] of the static node this state instantiates.
    pub fn path(&self) -> &NodePath {
        match self {
            RouteState::Route(r) => &r.path,
            RouteState::Stack(s) => &s.path,
            RouteState::Switch(s) => &s.path,
        }
    }

    /// Derive the **current** (shown) leaf: walk root → (Stack:
    /// history.top, Switch: selected) → leaf.
    ///
    /// `current` is computed, never stored — there is no marker field
    /// anywhere in [`RouteState`].
    pub fn current(&self) -> &RouteInstance {
        match self {
            RouteState::Route(r) => {
                if r.children.is_empty() {
                    r
                } else {
                    // A Route with children: descend through each child's
                    // active path to find the deepest current leaf. The
                    // first child that carries a non-trivial active path wins.
                    // (In practice a Route has one container child —
                    // Stack or Switch — that determines the active leaf.)
                    for child in &r.children {
                        let c = child.current();
                        if c.path != r.path {
                            return c;
                        }
                    }
                    r
                }
            }
            RouteState::Stack(s) => s
                .history
                .last()
                .expect("Stack history is never empty")
                .state
                .current(),
            RouteState::Switch(s) => s.branches[s.selected].current(),
        }
    }

    /// The active path from this node down to the current leaf, as the
    /// sequence of [`RouteState`] nodes visited (this node first, the
    /// leaf last).
    pub fn active_chain(&self) -> Vec<&RouteState> {
        let mut chain = vec![self];
        let mut node = self;
        loop {
            let next = match node {
                RouteState::Route(r) => {
                    if r.children.is_empty() {
                        break;
                    }
                    // Descend through the first container child.
                    match r
                        .children
                        .iter()
                        .find(|c| !matches!(c, RouteState::Route(ri) if ri.children.is_empty()))
                    {
                        Some(child) => child,
                        None => break,
                    }
                }
                RouteState::Stack(s) => &s.history.last().expect("non-empty").state,
                RouteState::Switch(s) => &s.branches[s.selected],
            };
            chain.push(next);
            node = next;
        }
        chain
    }
}

/// Clamp a `Switch`'s declared default to a legal branch index.
fn clamp_default(def: &SwitchDef, branch_count: usize) -> usize {
    let d = def.default_branch();
    if d < branch_count { d } else { 0 }
}
