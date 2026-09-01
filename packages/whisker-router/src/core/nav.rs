//! The [`Navigator`] — navigation operations over a [`RouteState`].
//!
//! A `Navigator` is a thin handle wrapping `&mut RouteState` +
//! `&CompiledTree`. Each operation is a mutation of `history` /
//! `selected`; [`RouteState::current`] recomputes afterward.
//!
//! | Op | Effect |
//! | --- | --- |
//! | [`navigate`](Navigator::navigate) | reveal the destination; unwind to an identical retained entry or push when absent; a group-only destination switches/reselects its branch |
//! | [`push`](Navigator::push) | resolve a screen and always append a fresh entry, selecting its branch |
//! | [`back`](Navigator::back) | pop the top of the **deepest non-trivial `Stack`** (history > 1) on the active path; `Switch` selection is never popped; tab-root with nothing to pop → no-op |
//! | [`replace`](Navigator::replace) | swap the **top** of the current stack with the target; same stack only (else [`NavError::CrossStack`]) |
//! | [`pop_to`](Navigator::pop_to) | pop the current stack until the target is the top; same stack only |
//! | [`reset`](Navigator::reset) | *global*: rebuild the whole state onto one clean path to `target` — select `Switch`es toward it and collapse **every** `Stack` to a single entry (no back history anywhere) |

use super::location::{Destination, Location};
use super::resolve::{self, ResolveError, Scope};
use super::state::{RouteInstance, RouteState, StackEntry, StackState};
use super::tree::{CompiledTree, NodePath, ReselectBehavior, RouteTree};

/// An error from an operation that cannot be expressed as a clean
/// mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive] // new failure modes (e.g. NothingToPop) can be added later
pub enum NavError {
    /// The destination is not an absolute router path.
    InvalidDestination,
    /// The target id/url matched no node in the tree.
    NoSuchTarget,
    /// More than one equally relevant placement matched an unqualified URL.
    AmbiguousRoute,
    /// The operation requires a screen, but the destination names only a
    /// route group.
    ExpectedRoute,
    /// `replace` / `pop_to` resolved to a node that is **not in the
    /// current stack**. These operations are same-stack only; crossing a
    /// `Switch` has no clean meaning, so it is a hard error (use
    /// `navigate` to cross branches).
    CrossStack,
    /// `pop_to` target is not present anywhere in the current stack's
    /// history.
    NotInStack,
    /// `back` had nothing to pop (at a stack/tab root). A no-op outcome,
    /// surfaced as an error so the verb's result is uniform with the others.
    NothingToPop,
}

/// A handle bundling the static tree with the mutable state, exposing
/// the navigation verbs.
pub struct Navigator<'a> {
    tree: &'a CompiledTree,
    state: &'a mut RouteState,
}

impl<'a> Navigator<'a> {
    /// Wrap a tree + mutable state.
    pub fn new(tree: &'a CompiledTree, state: &'a mut RouteState) -> Self {
        Navigator { tree, state }
    }

    /// The current (shown) leaf instance.
    pub fn current(&self) -> &RouteInstance {
        self.state.current()
    }

    /// The [`NodePath`] of the current leaf.
    pub fn current_path(&self) -> NodePath {
        self.state.current().path.clone()
    }

    /// Make `url` active. Existing identical entries are revealed by
    /// unwinding; absent entries are pushed. A group-only target switches to
    /// that branch, or pops it to its root when reselected.
    pub fn navigate(&mut self, url: &str) -> Result<(), NavError> {
        let destination = Destination::parse(url).ok_or(NavError::InvalidDestination)?;
        if let Some(group) = self.resolve_group_destination(&destination)? {
            return self.navigate_group(&group);
        }
        let current = self.current_path();
        let matched = resolve::resolve_route(self.tree, url, Some(&current)).map_err(nav_error)?;
        self.navigate_to_path(
            &matched.path,
            matched.params,
            destination.location,
            ForwardMode::Navigate,
        );
        Ok(())
    }

    /// Always append a fresh screen entry for `url`. A qualified destination
    /// selects that group before displaying the new entry.
    pub fn push(&mut self, url: &str) -> Result<(), NavError> {
        let destination = Destination::parse(url).ok_or(NavError::InvalidDestination)?;
        if self.resolve_group_destination(&destination)?.is_some() {
            return Err(NavError::ExpectedRoute);
        }
        let current = self.current_path();
        let matched = resolve::resolve_route(self.tree, url, Some(&current)).map_err(nav_error)?;
        self.navigate_to_path(
            &matched.path,
            matched.params,
            destination.location,
            ForwardMode::Push,
        );
        Ok(())
    }

    /// Navigate to `url` within an explicit [`Scope`]. See [`Scope`].
    pub fn navigate_within(&mut self, url: &str, scope: &Scope) -> Result<(), NavError> {
        let destination = Destination::parse(url).ok_or(NavError::InvalidDestination)?;
        if self.resolve_group_destination(&destination)?.is_some() {
            return Err(NavError::ExpectedRoute);
        }
        let current = self.current_path();
        let matched = resolve::resolve_route_within(self.tree, url, Some(&current), scope)
            .map_err(nav_error)?;
        self.navigate_to_path(
            &matched.path,
            matched.params,
            destination.location,
            ForwardMode::Navigate,
        );
        Ok(())
    }

    /// The mechanical part of `navigate`: walk root→`dest`, selecting
    /// `Switch` branches and pushing `Stack` children, revealing buried
    /// intermediate containers.
    fn navigate_to_path(
        &mut self,
        dest: &NodePath,
        params: std::collections::BTreeMap<String, String>,
        location: Location,
        mode: ForwardMode,
    ) {
        let tree = self.tree;
        walk_navigate(tree, self.state, dest, 0, &params, &location, mode);
    }

    fn resolve_group_destination(
        &self,
        destination: &Destination,
    ) -> Result<Option<NodePath>, NavError> {
        let groups = self
            .tree
            .group_paths_matching_url(&destination.qualified_pathname);
        match groups.as_slice() {
            [] => Ok(None),
            [group] => Ok(Some(group.clone())),
            _ => Err(NavError::AmbiguousRoute),
        }
    }

    fn navigate_group(&mut self, group: &NodePath) -> Result<(), NavError> {
        if group.is_ancestor_of(&self.current_path()) {
            if reselect_behavior(self.tree, group) == ReselectBehavior::PopToRoot {
                if let Some(group_state) = state_at_path_mut(self.state, group) {
                    pop_first_stack_to_root(group_state);
                }
            }
        } else {
            activate_container(self.tree, self.state, group, 0);
        }
        Ok(())
    }

    /// Pop the top of the deepest non-trivial `Stack` on the active path.
    /// [`NavError::NothingToPop`] when there is nothing poppable (e.g. at a
    /// tab root) — a no-op surfaced as an error for a uniform verb result.
    pub fn back(&mut self) -> Result<(), NavError> {
        if back_at(self.state) {
            Ok(())
        } else {
            Err(NavError::NothingToPop)
        }
    }

    /// Swap the **top** of the current stack with `target`. Same stack
    /// only: if `target` resolves outside the current stack,
    /// [`NavError::CrossStack`] is returned and the state is unchanged.
    pub fn replace(&mut self, url: &str) -> Result<(), NavError> {
        let destination = Destination::parse(url).ok_or(NavError::InvalidDestination)?;
        if self.resolve_group_destination(&destination)?.is_some() {
            return Err(NavError::ExpectedRoute);
        }
        let current = self.current_path();
        let matched = resolve::resolve_route(self.tree, url, Some(&current)).map_err(nav_error)?;
        let dest = matched.path;

        let stack_path = deepest_active_stack_path(self.state).ok_or(NavError::CrossStack)?;
        // The destination must be a *direct child* of the current stack
        // (same stack only).
        if !is_child_of(&stack_path, &dest) {
            return Err(NavError::CrossStack);
        }
        let stack = active_stack_mut(self.state).expect("stack exists");
        let entry = make_entry(self.tree, &dest, matched.params, destination.location);
        let top = stack.history.len() - 1;
        stack.history[top] = entry;
        Ok(())
    }

    /// Pop the current stack until `target` is the top. Same stack
    /// only. Errors with [`NavError::CrossStack`] if the target is not a
    /// child of the current stack, or [`NavError::NotInStack`] if no
    /// matching entry exists in its history.
    pub fn pop_to(&mut self, url: &str) -> Result<(), NavError> {
        let current = self.current_path();
        let dest =
            resolve::resolve(self.tree, url, Some(&current)).ok_or(NavError::NoSuchTarget)?;
        let stack_path = deepest_active_stack_path(self.state).ok_or(NavError::CrossStack)?;
        if !is_child_of(&stack_path, &dest) {
            return Err(NavError::CrossStack);
        }
        let stack = active_stack_mut(self.state).expect("stack exists");
        let idx = stack
            .history
            .iter()
            .rposition(|e| e.child == dest)
            .ok_or(NavError::NotInStack)?;
        stack.history.truncate(idx + 1);
        Ok(())
    }

    /// **Reset the whole navigation state** onto a single clean path to the
    /// route matched by `url` (the logout / clear-everything case). Unlike
    /// the other verbs this is *global*, not same-stack: `url` is resolved
    /// across the entire tree (like `navigate`), every `Switch` is selected
    /// toward the target, and **every `Stack` collapses to a single entry**
    /// so no back history survives anywhere — on the path to the target or in
    /// any other branch. Errors only with [`NavError::NoSuchTarget`].
    pub fn reset(&mut self, url: &str) -> Result<(), NavError> {
        let destination = Destination::parse(url).ok_or(NavError::InvalidDestination)?;
        if self.resolve_group_destination(&destination)?.is_some() {
            return Err(NavError::ExpectedRoute);
        }
        let current = self.current_path();
        let matched = resolve::resolve_route(self.tree, url, Some(&current)).map_err(nav_error)?;
        *self.state = RouteState::focused_at(
            self.tree,
            &matched.path,
            matched.params,
            destination.location,
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForwardMode {
    Navigate,
    Push,
}

fn nav_error(error: ResolveError) -> NavError {
    match error {
        ResolveError::NotFound => NavError::NoSuchTarget,
        ResolveError::Ambiguous => NavError::AmbiguousRoute,
    }
}

/// Recursively drive `state` toward `dest`, starting at child-index
/// `depth` of `dest`.
///
/// At each level, `dest.0[depth]` names the child to move toward.
fn walk_navigate(
    tree: &CompiledTree,
    state: &mut RouteState,
    dest: &NodePath,
    depth: usize,
    params: &std::collections::BTreeMap<String, String>,
    location: &Location,
    mode: ForwardMode,
) {
    if depth == dest.0.len() {
        // `state` is the destination node itself. If it is a leaf, set
        // its params; if it is a container we've already arrived (the
        // caller having pushed/selected us here) — nothing more to do.
        if let RouteState::Route(r) = state {
            r.params = params.clone();
            r.location = location.clone();
        }
        return;
    }

    let toward = dest.0[depth];
    match state {
        RouteState::Switch(s) => {
            // Select the branch toward dest, then descend into it.
            s.selected = toward;
            walk_navigate(
                tree,
                &mut s.branches[toward],
                dest,
                depth + 1,
                params,
                location,
                mode,
            );
        }
        RouteState::Stack(s) => {
            let child_path = s.path.child(toward);
            // The remaining destination beyond this stack's child.
            let arrives_at_leaf = depth + 1 == dest.0.len();

            if arrives_at_leaf {
                if mode == ForwardMode::Navigate {
                    if let Some(index) = s
                        .history
                        .iter()
                        .rposition(|entry| entry_matches(entry, &child_path, params, location))
                    {
                        s.history.truncate(index + 1);
                        return;
                    }
                }
                s.history.push(make_entry(
                    tree,
                    &child_path,
                    params.clone(),
                    location.clone(),
                ));
            } else {
                // The destination passes *through* a container child of
                // this stack. Reveal a buried instance of that container
                // if present (pop entries above it), preserving its
                // retained `selected`/`history`; otherwise push a fresh
                // one. Then descend.
                let i = reveal_or_push(tree, s, &child_path);
                walk_navigate(
                    tree,
                    &mut s.history[i].state,
                    dest,
                    depth + 1,
                    params,
                    location,
                    mode,
                );
            }
        }
        RouteState::Route(r) => {
            // A Route with children: descend through the matching child.
            if !r.children.is_empty() && depth < dest.0.len() {
                let toward = dest.0[depth];
                if toward < r.children.len() {
                    walk_navigate(
                        tree,
                        &mut r.children[toward],
                        dest,
                        depth + 1,
                        params,
                        location,
                        mode,
                    );
                }
            }
        }
    }
}

/// Select/reveal the container at `dest` without changing the retained stack
/// inside it.
fn activate_container(tree: &CompiledTree, state: &mut RouteState, dest: &NodePath, depth: usize) {
    if depth == dest.0.len() {
        return;
    }
    let toward = dest.0[depth];
    match state {
        RouteState::Switch(s) => {
            s.selected = toward;
            activate_container(tree, &mut s.branches[toward], dest, depth + 1);
        }
        RouteState::Stack(s) => {
            let child_path = s.path.child(toward);
            let index = reveal_or_push(tree, s, &child_path);
            activate_container(tree, &mut s.history[index].state, dest, depth + 1);
        }
        RouteState::Route(r) => {
            // Route with children: descend into the matching child.
            if !r.children.is_empty() && depth < dest.0.len() {
                let toward = dest.0[depth];
                if toward < r.children.len() {
                    activate_container(tree, &mut r.children[toward], dest, depth + 1);
                }
            }
        }
    }
}

/// Reveal a buried container instance of `child_path` in `stack`, or
/// push a fresh one. Returns the history index to descend into.
///
/// "Reveal" = if an existing entry for this exact container child is in
/// the history, pop everything above it (so it becomes the active top)
/// and keep its retained nested state. This is the buried-tabs case:
/// `navigate` to a tab route from an outside route reveals the tabs
/// `Switch` rather than minting a second one.
fn reveal_or_push(tree: &CompiledTree, stack: &mut StackState, child_path: &NodePath) -> usize {
    if let Some(i) = stack.history.iter().position(|e| e.child == *child_path) {
        // Reveal: drop everything above the existing container entry.
        stack.history.truncate(i + 1);
        i
    } else {
        let state_child = RouteState::initial_at(tree, child_path);
        stack.history.push(StackEntry {
            child: child_path.clone(),
            state: state_child,
        });
        stack.history.len() - 1
    }
}

/// Build a [`StackEntry`] for a leaf or container `dest` with `params`.
fn make_entry(
    tree: &CompiledTree,
    dest: &NodePath,
    params: std::collections::BTreeMap<String, String>,
    location: Location,
) -> StackEntry {
    let mut state = RouteState::initial_at(tree, dest);
    if let RouteState::Route(r) = &mut state {
        r.params = params;
        r.location = location;
    }
    StackEntry {
        child: dest.clone(),
        state,
    }
}

fn entry_matches(
    entry: &StackEntry,
    dest: &NodePath,
    params: &std::collections::BTreeMap<String, String>,
    location: &Location,
) -> bool {
    entry.child == *dest
        && matches!(
            &entry.state,
            RouteState::Route(route)
                if route.params == *params && route.location == *location
        )
}

fn reselect_behavior(tree: &CompiledTree, group: &NodePath) -> ReselectBehavior {
    for depth in (0..group.len()).rev() {
        let ancestor = NodePath(group.0[..depth].to_vec());
        if let Some(RouteTree::Switch(def, _)) = tree.node_at(&ancestor) {
            return def.reselect;
        }
    }
    ReselectBehavior::PopToRoot
}

fn state_at_path_mut<'a>(state: &'a mut RouteState, path: &NodePath) -> Option<&'a mut RouteState> {
    let mut current = state;
    for &index in &path.0 {
        current = match current {
            RouteState::Switch(switch) => switch.branches.get_mut(index)?,
            RouteState::Route(route) => route.children.get_mut(index)?,
            RouteState::Stack(stack) => {
                let entry = stack
                    .history
                    .iter_mut()
                    .rev()
                    .find(|entry| entry.child.0.last() == Some(&index))?;
                &mut entry.state
            }
        };
    }
    Some(current)
}

fn pop_first_stack_to_root(state: &mut RouteState) -> bool {
    match state {
        RouteState::Stack(stack) => {
            let changed = stack.history.len() > 1;
            stack.history.truncate(1);
            changed
        }
        RouteState::Switch(switch) => {
            pop_first_stack_to_root(&mut switch.branches[switch.selected])
        }
        RouteState::Route(route) => route.children.iter_mut().any(pop_first_stack_to_root),
    }
}

/// Pop the deepest non-trivial `Stack` (history > 1) on the active
/// path. Returns whether anything was popped.
///
/// We recurse to the bottom of the active chain first, so the
/// **deepest** poppable stack wins; a higher stack only pops if no
/// deeper one could.
fn back_at(state: &mut RouteState) -> bool {
    match state {
        RouteState::Route(r) => r.children.iter_mut().any(back_at),
        RouteState::Switch(s) => {
            let sel = s.selected;
            back_at(&mut s.branches[sel])
        }
        RouteState::Stack(s) => {
            let top = s.history.len() - 1;
            // Try to pop deeper first.
            if back_at(&mut s.history[top].state) {
                return true;
            }
            // Otherwise pop this stack if non-trivial.
            if s.history.len() > 1 {
                s.history.pop();
                true
            } else {
                false
            }
        }
    }
}

/// The path of the deepest `Stack` on the active path (the stack whose
/// top is/leads to the current leaf). `None` if there is no stack on the
/// active path at all.
fn deepest_active_stack_path(state: &RouteState) -> Option<NodePath> {
    let mut found: Option<NodePath> = None;
    for node in state.active_chain() {
        if let RouteState::Stack(s) = node {
            found = Some(s.path.clone());
        }
    }
    found
}

/// A mutable reference to the deepest `Stack` on the active path.
fn active_stack_mut(state: &mut RouteState) -> Option<&mut StackState> {
    fn go(state: &mut RouteState) -> Option<&mut StackState> {
        match state {
            RouteState::Route(r) => {
                // A *leaf* Route has no stack below it, but a layout/group
                // Route wraps a Switch/Stack/Route child and must be
                // descended into — exactly as `active_chain` /
                // `deepest_active_stack_path` do. The three have to agree, or
                // `replace`/`reset`/`pop_to` find a path with no `&mut` stack.
                if r.children.is_empty() {
                    return None;
                }
                let idx = r
                    .children
                    .iter()
                    .position(|c| !matches!(c, RouteState::Route(ri) if ri.children.is_empty()))?;
                go(&mut r.children[idx])
            }
            RouteState::Switch(s) => {
                let sel = s.selected;
                go(&mut s.branches[sel])
            }
            RouteState::Stack(s) => {
                let top = s.history.len() - 1;
                // Prefer a deeper stack if one exists on the active path.
                let has_deeper = deepest_active_stack_path(&s.history[top].state).is_some();
                if has_deeper {
                    go(&mut s.history[top].state)
                } else {
                    Some(s)
                }
            }
        }
    }
    go(state)
}

/// Whether `child` is a direct child of `parent` (parent path + one
/// index).
fn is_child_of(parent: &NodePath, child: &NodePath) -> bool {
    child.0.len() == parent.0.len() + 1 && child.0[..parent.0.len()] == parent.0[..]
}
