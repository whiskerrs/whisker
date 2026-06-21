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

/// A target to resolve: either a nav-target id ([`RouteDef::id`]) or a
/// full URL.
///
/// [`RouteDef::id`]: super::tree::RouteDef::id
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Match by nav-target identity (the deduped shared-route case).
    Id(String),
    /// Match by derived full URL.
    Url(String),
}

impl Target {
    /// A target by nav-target id.
    pub fn id(id: impl Into<String>) -> Self {
        Target::Id(id.into())
    }

    /// A target by URL.
    pub fn url(url: impl Into<String>) -> Self {
        Target::Url(url.into())
    }
}

/// An explicit resolution-scope override hook (`within(scope)`).
///
/// This is the rare cross-branch case (`route::post(42).within(
/// scope::search)`). The **API surface is provided** so callers can be
/// written against it, but full behaviour is deferred to a later phase
/// per the design doc's open items; [`resolve_within`] currently
/// restricts the candidate set to the scope subtree and then applies the
/// ordinary deepest-common-ancestor rule within it.
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

/// All static paths matching `target`, in declaration order.
fn candidates(tree: &CompiledTree, target: &Target) -> Vec<NodePath> {
    match target {
        Target::Id(id) => tree.paths_with_route_id(id),
        Target::Url(url) => tree.paths_with_url(url),
    }
}

/// Resolve `target` relative to `current` (the path of the current
/// leaf, or `None` for cold start).
///
/// Returns the chosen candidate's [`NodePath`], or `None` if nothing
/// matches.
pub fn resolve(
    tree: &CompiledTree,
    target: &Target,
    current: Option<&NodePath>,
) -> Option<NodePath> {
    let cands = candidates(tree, target);
    if cands.is_empty() {
        return None;
    }

    match current {
        // Cold start: resolve from the root ⇒ pure declaration order.
        None => Some(cands[0].clone()),
        Some(cur) => {
            // Walk up from the current leaf's ancestors (deepest first):
            // current itself, then its parent, ... up to the root.
            // The first ancestor whose subtree contains a candidate
            // wins; declaration order (candidates are pre-ordered)
            // breaks ties within it.
            for depth in (0..=cur.len()).rev() {
                let ancestor = NodePath(cur.0[..depth].to_vec());
                if let Some(found) = cands.iter().find(|c| ancestor.is_ancestor_of(c)) {
                    return Some(found.clone());
                }
            }
            // Unreachable in a well-formed tree (root is an ancestor of
            // everything), but stay total.
            Some(cands[0].clone())
        }
    }
}

/// Resolve `target` restricted to `scope`'s subtree, then by the
/// ordinary relative rule within it. See [`Scope`] (deferred behaviour).
pub fn resolve_within(
    tree: &CompiledTree,
    target: &Target,
    current: Option<&NodePath>,
    scope: &Scope,
) -> Option<NodePath> {
    let cands: Vec<NodePath> = candidates(tree, target)
        .into_iter()
        .filter(|c| scope.root.is_ancestor_of(c))
        .collect();
    if cands.is_empty() {
        return None;
    }
    // Prefer a candidate sharing the deepest ancestor with current,
    // falling back to declaration order within the scope.
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
