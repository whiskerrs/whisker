//! The user-facing rendering components: [`Router`], [`Outlet`],
//! [`Stack`], [`Switch`], and the tab chrome ([`Tabs`] / [`TabBar`]).
//!
//! [`Router`] is the root: it publishes the [`RouterHandle`] into context
//! and renders the route tree. [`Outlet`] renders "the active child of
//! the container I'm in" — its anchor (which container) comes from a
//! context value an enclosing [`Layout`](crate::render::Layout) /
//! [`Tabs`] sets, defaulting to the tree root. [`Stack`] / [`Switch`]
//! render an explicit subtree path for advanced compositions; most apps
//! only need [`Router`] + [`Outlet`].
//!
//! All four stand on the recursive [`mount_node`](crate::render::node)
//! engine; they differ only in *which* [`NodePath`] they hand it.

use whisker::css::{Display, FlexDirection};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::{Children, component, computed, provide_context, render, use_context};

use crate::core::NodePath;
use crate::render::handle::{RouterHandle, use_navigator};
use crate::render::node::mount_node;

/// The router's screen-spanning root element. [`Router`] publishes it so
/// the [`SwipeBack`](crate::render::SwipeBack) gesture can bind its touch
/// handlers to an element that actually covers the viewport (a phantom
/// slot has no extent and would never be hit).
#[derive(Clone)]
pub struct RouterRoot(pub Element);

/// The container path an [`Outlet`] renders. Published by [`Router`] (the
/// root) and overridden by a [`Layout`] / [`Tabs`] so a nested `Outlet`
/// renders that layout's container.
#[derive(Clone)]
pub struct OutletAnchor(pub NodePath);

/// Root router component: publishes `handle` into context and renders the
/// whole active route tree.
///
/// # Responsibility split (one draw path)
///
/// `Router` deliberately **does not draw the route tree itself**. Its job
/// is exactly: publish the context (handle, root [`OutletAnchor`],
/// [`RouterRoot`]), create the positioned root `view`, and render its
/// `children` into it. The tree is drawn **once** by an `Outlet`-family
/// component you place as a child — a bare [`Outlet`] (anchored at root),
/// a [`Stack`] / [`Switch`] at an explicit path, or a [`Tabs`] /
/// [`Layout`] that draws a container with chrome. This keeps every node on
/// a single mount path: putting both `Router`'s own draw *and* a `Tabs`
/// child would mount the shared subtree twice.
///
/// ```ignore
/// render! {
///     Router(handle: handle.clone()) {
///         Tabs(path: switch_path, items: ...)   // draws the Switch + bar
///         SwipeBack {}
///     }
/// }
/// ```
#[component]
pub fn router(handle: RouterHandle, children: Children) -> Element {
    provide_context(handle.clone());
    provide_context(OutletAnchor(NodePath::root()));

    // A real, screen-spanning root so transitions have a positioned
    // container (wrappers are `position: absolute`) and the swipe-back
    // gesture has something to bind to.
    //
    // The `children()` slot is bundled behind a phantom; appending that
    // phantom directly would hoist the children with NO column container,
    // and Lynx defaults a style-less container to `flex-direction: row`
    // (see memory `lynx_view_flex_direction_default`) — collapsing the
    // children horizontally (the tab content eats the row, side-effect
    // gesture/marker views shrink to 0). So `root` itself is the real
    // `flex-direction: column` container the children mount into directly.
    // Build the root view EMPTY first, then publish the `RouterRoot`
    // context, and only THEN mount the children into it. Ordering matters:
    // `children()` (e.g. a `SwipeBack` that reads `RouterRoot` to bind its
    // gesture) mounts at the point it is rendered, so it must run *after*
    // `provide_context(RouterRoot(root))` — otherwise it sees `None` and the
    // gesture is silently never installed (the iOS swipe-back bug).
    let root = render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        ).raw("position", "relative")) {}
    };
    provide_context(RouterRoot(root));
    // Mount the children now that `RouterRoot` is in context. The tree is
    // drawn by `children` (an Outlet / Tabs / Stack), NOT here — drawing
    // root ourselves *and* letting a child draw the same subtree would
    // double-mount it. Appending them under the column `root` keeps the
    // `flex-direction: column` container (a style-less phantom would hoist
    // them into Lynx's default `row`).
    whisker::runtime::view::append_child(root, whisker::runtime::view::mount_children(&children));

    // Prime the device screen corner radius at router init (it feeds the
    // constant per-screen clip). Run in `on_mount` so the host Activity has
    // a chance to attach; if it isn't resolvable yet the call falls back to
    // the default and the first back gesture retries (idempotent once a
    // value is installed).
    whisker::on_mount(crate::render::gesture::try_fetch_device_corner_radius);

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
///         view(..) {
///             view(style: css!(flex_grow: 1.0)) { Outlet {} }
///             MyCustomTabBar {}
///         }
///     }
/// }
/// ```
#[component]
pub fn layout(path: NodePath, children: Children) -> Element {
    provide_context(OutletAnchor(path.clone()));
    render! { children() }
}

/// Reactive read of the selected branch index of the `Switch` at `path`.
///
/// The `use_active_tab()` helper of the design doc, generalised to take
/// the switch's path (the macro will later infer it). Returns `0` when
/// the node is not a live switch (shouldn't happen for a correct path).
pub fn use_active_tab(path: NodePath) -> ReadSignal<usize> {
    let handle = use_navigator();
    let sel = handle.selected_at(path);
    computed(move || sel.get().unwrap_or(0))
}
