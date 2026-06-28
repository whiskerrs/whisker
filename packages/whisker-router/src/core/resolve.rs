//! Relative resolution: which **instance** of an (often shared) target
//! does a [`navigate`](super::nav::Navigator::navigate) hit?
//!
//! > Among nodes matching the target, pick the one whose path shares the
//! > **deepest common ancestor with the current position**. Break ties
//! > by **declaration order** (first defined wins).
//!
//! Operationally ([`resolve`]): walk up from the current leaf to the
//! root; the first (deepest) ancestor whose subtree contains a match
//! resolves it; within that subtree, declaration (pre-order) order
//! breaks ties. Cold start (no current) resolves from the root, i.e.
//! pure declaration order.

use super::tree::{CompiledTree, NodePath};

/// An explicit resolution-scope override hook (`within(scope)`).
///
/// This is the rare cross-branch case. The **API surface is provided**
/// so callers can be written against it, but full behaviour is deferred
/// to a later phase; [`resolve_within`] currently restricts the candidate
/// set to the scope subtree and then applies the ordinary
/// deepest-common-ancestor rule within it.
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
/// Group segments in the URL are optional: `"/detail/42"` and
/// `"/(home)/detail/42"` both resolve against the pattern
/// `/(home)/detail/:id`.
///
/// Returns the chosen candidate's [`NodePath`], or `None` if nothing
/// matches.
pub fn resolve(tree: &CompiledTree, url: &str, current: Option<&NodePath>) -> Option<NodePath> {
    // Prefer a leaf **screen** matching the URL.
    if let Some(found) = pick_relative(&tree.paths_matching_url(url), current) {
        return Some(found);
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

/// Pick from `cands` by the deepest-common-ancestor-with-`current` rule
/// (declaration order breaks ties); cold start (no current) takes the first.
/// `None` when `cands` is empty.
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
    let cands: Vec<NodePath> = tree
        .paths_matching_url(url)
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
