//! Relative resolution: which **instance** of an (often shared) target
//! does a [`navigate`](super::nav::Navigator::navigate) hit?
//!
//! > Among nodes matching the target, pick the one whose path shares the
//! > **deepest common ancestor with the current position**. An equal tie is
//! > ambiguous and must be qualified with a route group.
//!
//! Cold resolution uses the tree's configured initial state as its current
//! position, so a shared public route prefers the initial `Switch` branch.

use super::state::RouteState;
use super::tree::{CompiledTree, NodePath, RouteMatch};

/// Failure to choose one concrete placement for a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveError {
    NotFound,
    Ambiguous,
}

/// An explicit resolution-scope override hook (`within(scope)`).
///
/// This is the rare cross-branch case. [`resolve_within`] restricts the
/// candidate set to the scope subtree and then applies the ordinary
/// deepest-common-ancestor rule within it — it does not yet implement a
/// scope-specific resolution rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// The subtree to resolve within (a container node's path).
    pub root: NodePath,
}

impl Scope {
    /// A scope rooted at `path`.
    pub fn at(path: NodePath) -> Self {
        Scope { root: path }
    }
}

/// Resolve a URL relative to `current` (the path of the current leaf,
/// or `None` for cold start).
///
/// An unqualified public URL such as `"/detail/42"` is resolved relative to
/// the active branch. A qualified destination such as
/// `"/(home)/detail/42"` selects that exact placement.
///
/// Returns the chosen candidate's [`NodePath`], or `None` if nothing
/// matches.
pub fn resolve(tree: &CompiledTree, url: &str, current: Option<&NodePath>) -> Option<NodePath> {
    match resolve_route(tree, url, current) {
        Ok(found) => return Some(found.path),
        Err(ResolveError::Ambiguous) => return None,
        Err(ResolveError::NotFound) => {}
    }
    // Fallback: the URL named a **container** (e.g. a bare `/(group)` whose
    // group has no index `""` screen) — resolve to the index leaf inside the
    // chosen container so it is still a navigable destination.
    let container = pick_relative(&tree.container_paths_matching_url(url), current)?;
    Some(
        super::state::RouteState::initial_at(tree, &container)
            .current()
            .path
            .clone(),
    )
}

/// Resolve a leaf route together with the params captured for that exact
/// placement. An unqualified shared route prefers the current branch. If
/// several candidates are equally near, the destination is ambiguous rather
/// than silently choosing declaration order.
pub(crate) fn resolve_route(
    tree: &CompiledTree,
    url: &str,
    current: Option<&NodePath>,
) -> Result<RouteMatch, ResolveError> {
    let initial_current = current
        .is_none()
        .then(|| RouteState::initial(tree).current().path.clone());
    pick_relative_match(
        tree.route_matches(url),
        current.or(initial_current.as_ref()),
    )
}

fn pick_relative_match(
    candidates: Vec<RouteMatch>,
    current: Option<&NodePath>,
) -> Result<RouteMatch, ResolveError> {
    if candidates.is_empty() {
        return Err(ResolveError::NotFound);
    }
    let Some(current) = current else {
        return Ok(candidates.into_iter().next().expect("non-empty"));
    };

    let best_depth = candidates
        .iter()
        .map(|candidate| common_prefix_len(&candidate.path, current))
        .max()
        .expect("non-empty");
    let mut nearest = candidates
        .into_iter()
        .filter(|candidate| common_prefix_len(&candidate.path, current) == best_depth);
    let found = nearest.next().expect("best candidate exists");
    if nearest.next().is_some() {
        Err(ResolveError::Ambiguous)
    } else {
        Ok(found)
    }
}

fn common_prefix_len(a: &NodePath, b: &NodePath) -> usize {
    a.0.iter().zip(&b.0).take_while(|(x, y)| x == y).count()
}

/// Pick a legacy container candidate relative to `current`. Container
/// resolution is kept for the low-level [`resolve`] compatibility API;
/// navigation verbs handle qualified group destinations directly.
fn pick_relative(cands: &[NodePath], current: Option<&NodePath>) -> Option<NodePath> {
    if cands.is_empty() {
        return None;
    }
    match current {
        None => Some(cands[0].clone()),
        Some(cur) => {
            for depth in (0..=cur.len()).rev() {
                let ancestor = NodePath(cur.0[..depth].to_vec());
                if let Some(found) = cands.iter().find(|c| ancestor.is_ancestor_of(c)) {
                    return Some(found.clone());
                }
            }
            Some(cands[0].clone())
        }
    }
}

/// Resolve `url` restricted to `scope`'s subtree, then by the ordinary
/// relative rule within it. See [`Scope`].
pub fn resolve_within(
    tree: &CompiledTree,
    url: &str,
    current: Option<&NodePath>,
    scope: &Scope,
) -> Option<NodePath> {
    match resolve_route_within(tree, url, current, scope) {
        Ok(found) => return Some(found.path),
        Err(ResolveError::Ambiguous) => return None,
        Err(ResolveError::NotFound) => {}
    }
    let cands: Vec<NodePath> = tree
        .container_paths_matching_url(url)
        .into_iter()
        .filter(|c| scope.root.is_ancestor_of(c))
        .collect();
    if cands.is_empty() {
        return None;
    }
    match current {
        None => Some(cands[0].clone()),
        Some(cur) => {
            for depth in (0..=cur.len()).rev() {
                let ancestor = NodePath(cur.0[..depth].to_vec());
                if !scope.root.is_ancestor_of(&ancestor) && ancestor != scope.root {
                    continue;
                }
                if let Some(found) = cands.iter().find(|c| ancestor.is_ancestor_of(c)) {
                    return Some(found.clone());
                }
            }
            Some(cands[0].clone())
        }
    }
}

pub(crate) fn resolve_route_within(
    tree: &CompiledTree,
    url: &str,
    current: Option<&NodePath>,
    scope: &Scope,
) -> Result<RouteMatch, ResolveError> {
    let candidates = tree
        .route_matches(url)
        .into_iter()
        .filter(|candidate| scope.root.is_ancestor_of(&candidate.path))
        .collect();
    pick_relative_match(candidates, current)
}
