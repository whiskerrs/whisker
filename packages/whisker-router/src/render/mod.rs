//! # Reactive rendering of the new router core (phase 2)
//!
//! This module draws the Phase-1 [`RouteTree`](crate::core::RouteTree) /
//! [`RouteState`](crate::core::RouteState) core in the Whisker reactive
//! runtime: a signal-backed [`RouterHandle`] + [`use_navigator`], the
//! [`Outlet`] / [`Stack`] / [`Switch`] renderers, the [`Tabs`] chrome,
//! float-`Tween` transitions (via `whisker-animation`, **not** Lynx's
//! animator), and the interactive back gestures ([`SwipeBack`] for iOS
//! edge swipe, [`AndroidPredictiveBack`] for Android 13+ predictive back)
//! — both driving the same coordinated two-screen scrub.
//!
//! The id → component map is a **hand-written** [`RouteRegistry`] in this
//! phase; the `routes!` macro will generate it in phase 3.
//!
//! ## The signal model
//!
//! [`RouterHandle`] owns the immutable [`CompiledTree`](crate::core::CompiledTree),
//! the [`RouteRegistry`], and a single `RwSignal<RouteState>`. Every verb
//! (`navigate` / `select` / `back` / `replace` / `pop_to` / `reset`)
//! clones the state, runs the Phase-1 [`Navigator`](crate::core::Navigator)
//! op, and writes the signal back. `current` is a `computed`, never
//! stored.
//!
//! ## Fine-grained re-render
//!
//! Each container renders from its **own slice** of the state
//! ([`RouterHandle::slice_at`]); because `computed` memoises by
//! `PartialEq` (`RouteState: Eq`), an op that doesn't change a given
//! container's subtree produces an unchanged slice and that container's
//! mount effect does not re-run. So pushing into tab A's stack does not
//! re-render tab B, and backgrounded screens stay mounted (frozen via
//! [`Owner::pause`](whisker::runtime::reactive::Owner::pause)) — only the
//! affected leaf swaps.
//!
//! ## Usage (hand-written registry, no macro yet)
//!
//! ```ignore
//! use whisker_router::core::{CompiledTree, RouteTree, Target};
//! use whisker_router::render::*;
//!
//! let tree = CompiledTree::new(RouteTree::stack(vec![
//!     RouteTree::route("", "home"),
//!     RouteTree::route("detail/:id", "detail"),
//! ]));
//! let registry = RouteRegistry::new()
//!     .route("home",   |_| render! { Home {} })
//!     .route("detail", |inst| render! { Detail(id: inst.params.get("id").cloned().unwrap_or_default()) });
//! let handle = RouterHandle::new(tree, registry);
//!
//! render! {
//!     Router(handle: handle.clone()) {
//!         SwipeBack {}
//!     }
//! }
//! // inside a screen:  use_navigator().navigate(&Target::id("detail"));
//! ```

pub mod components;
pub mod gesture;
pub mod handle;
pub mod node;
pub mod registry;
pub mod tabs;
pub mod transition;

#[cfg(test)]
mod tests;

pub use components::{
    Layout, LayoutProps, OutletAnchor, Router, RouterProps, RouterRoot, Stack, StackProps, Switch,
    SwitchProps, use_active_tab,
};
pub use components::{Outlet, OutletProps};
pub use gesture::{
    AndroidPredictiveBack, AndroidPredictiveBackProps, DiagMarker, DiagMarkerProps, SwipeBack,
    SwipeBackProps,
};
pub use handle::{RouterHandle, StackBridge, provide_router, use_navigator};
pub use registry::{RenderFn, RouteRegistry, Transition};
pub use tabs::{TabBar, TabBarProps, TabItem, Tabs, TabsProps};
pub use transition::Role;
