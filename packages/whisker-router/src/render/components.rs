//! The user-facing rendering components: [`Router`], [`Outlet`],
//! [`Stack`], [`Switch`], and [`Layout`].
//!
//! [`Router`] is the root: it publishes the [`RouterHandle`] into context
//! and renders the route tree. [`Outlet`] renders "the active child of
//! the container I'm in" — its anchor (which container) comes from a
//! context value an enclosing [`Layout`] sets, defaulting to the tree
//! root. [`Stack`] / [`Switch`] render an explicit subtree path for
//! advanced compositions; most apps only need [`Router`] + [`Outlet`].
//!
//! All stand on the recursive [`mount_node`](crate::render::node) engine;
//! they differ only in *which* [`NodePath`] they hand it.

use whisker::css::{Display, FlexDirection, PositionKind};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::{Children, component, provide_context, render, use_context};

use crate::core::NodePath;
use crate::render::handle::{RouterHandle, use_navigator};
use crate::render::node::mount_node;
use crate::render::registry::RouteSet;

/// The container path an [`Outlet`] renders. Published by [`Router`] (the
/// root) and overridden by a [`Layout`] so a nested `Outlet`
/// renders that layout's container.
#[derive(Clone)]
pub struct OutletAnchor(pub NodePath);

/// Root router component: publishes `handle` into context and renders the
/// whole active route tree.
///
/// # Responsibility split (one draw path)
///
/// `Router` deliberately **does not draw the route tree itself**. Its job
/// is exactly: publish the context (handle and root [`OutletAnchor`]),
/// create the positioned root `View`, install the Host navigation driver,
/// and render its `children` into it. The tree is drawn **once** by an
/// `Outlet`-family
/// component you place as a child — a bare [`Outlet`] (anchored at root),
/// a [`Stack`] / [`Switch`] at an explicit path, or a [`Tabs`] /
/// [`Layout`] that draws a container with chrome. This keeps every node on
/// a single mount path: putting both `Router`'s own draw *and* a `Tabs`
/// child would mount the shared subtree twice.
///
/// ```ignore
/// render! {
///     Router(routes: routes! { Stack { ... } }) {
///         Outlet {}
///     }
/// }
/// ```
#[component]
pub fn router(routes: RouteSet, children: Children) -> Element {
    let handle = RouterHandle::new(routes.clone());
    provide_context(handle.clone());
    provide_context(OutletAnchor(NodePath::root()));

    // A real, screen-spanning root so transitions have a positioned
    // container (wrappers are `position: absolute`) and the swipe-back
    // gesture has something to bind to.
    //
    // The projected children are bundled behind a phantom, and appending that
    // phantom directly would hoist the children into a style-less container
    // with the default row direction, collapsing them horizontally. So
    // `root` itself is the `flex-direction: column` container they mount into.
    //
    // Build `root` empty so the platform navigation driver can bind to the
    // real viewport-filling element before projected route content mounts.
    let root = render! {
        View(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            position: PositionKind::Relative,
        )) {}
    };
    crate::render::platform_navigation::install(root, handle);
    // The tree is drawn by `children` (an Outlet / Tabs / Stack), NOT here —
    // drawing root ourselves *and* letting a child draw the same subtree
    // would double-mount it.
    whisker::runtime::view::append_child(root, whisker::runtime::view::mount_children(&children));

    root
}

/// Render the active child of the container at the current
/// [`OutletAnchor`] (defaults to the tree root).
///
/// Place an `Outlet` inside a custom [`Layout`] to draw chrome around the
/// router's content (the `_layout.tsx` equivalent).
#[component]
pub fn outlet() -> Element {
    let handle = use_navigator();
    let anchor = use_context::<OutletAnchor>()
        .map(|a| a.0)
        .unwrap_or_else(NodePath::root);
    mount_node(&handle, anchor)
}

/// Render the [`Stack`](crate::core::RouteTree::Stack) subtree at an
/// explicit `path`.
///
/// The lower-level primitive behind an `Outlet` that anchors on a stack;
/// reach for it when you are composing the tree by hand. Reads the
/// [`RouterHandle`] from context.
#[component]
pub fn stack(path: NodePath) -> Element {
    let handle = use_navigator();
    mount_node(&handle, path.clone())
}

/// Render the [`Switch`](crate::core::RouteTree::Switch) subtree at an
/// explicit `path` (all branches kept alive, `selected` toggled).
#[component]
pub fn switch(path: NodePath) -> Element {
    let handle = use_navigator();
    mount_node(&handle, path.clone())
}

/// A custom chrome wrapper around a container's [`Outlet`].
///
/// Sets the [`OutletAnchor`] to `path` so the `Outlet` in `children`
/// renders that container, then renders the children (your chrome + the
/// `Outlet`). This is the explicit `Layout(X)` of the design doc.
///
/// ```ignore
/// render! {
///     Layout(path: switch_path) {
///         View(..) {
///             View(style: css!(flex_grow: 1.0)) { Outlet {} }
///             MyCustomTabBar {}
///         }
///     }
/// }
/// ```
#[component]
pub fn layout(path: NodePath, children: Children) -> Element {
    provide_context(OutletAnchor(path.clone()));
    let projected = children.clone();
    render! { Fragment { { projected() } } }
}
