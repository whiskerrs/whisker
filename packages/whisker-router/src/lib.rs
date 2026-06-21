//! # whisker-router
//!
//! Declarative routing for Whisker, built on **two graphs**: a static
//! [`RouteTree`](core::RouteTree) describing the app's screen structure,
//! and a dynamic [`RouteState`](core::RouteState) the runtime mutates as
//! the user navigates. The shown screen, where a `navigate` lands, and
//! where a `back` returns are all *derived* from these two — there is no
//! hand-maintained route table or stored "current screen" pointer. See
//! [`docs/router-design.md`] for the model and the "why".
//!
//! ## The two layers
//!
//! - [`core`] — the **pure-logic** model (phase 1):
//!   [`RouteTree`](core::RouteTree) / [`CompiledTree`](core::CompiledTree),
//!   [`RouteState`](core::RouteState), and the [`Navigator`](core::Navigator)
//!   with the five operations (`navigate` / `select` / `back` / `replace` /
//!   `pop_to` / `reset`). No signals, no `Element` — unit-testable on its own.
//! - [`render`] — the **reactive rendering** of that core in the Whisker
//!   runtime (phase 2). A signal-backed [`RouterHandle`](render::RouterHandle)
//!   plus [`use_navigator`](render::use_navigator), the
//!   [`Outlet`](render::Outlet), [`Stack`](render::Stack) and
//!   [`Switch`](render::Switch) renderers, the [`Tabs`](render::Tabs) chrome,
//!   float-`Tween` transitions (via `whisker-animation`, not Lynx's animator),
//!   and the iOS [`SwipeBack`](render::SwipeBack) gesture.
//!
//! The route id → component mapping is a hand-written
//! [`RouteRegistry`](render::RouteRegistry) for now; the `routes!` macro
//! will generate it in a later phase.
//!
//! ## Minimal usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_router::core::{CompiledTree, RouteTree, Target};
//! use whisker_router::render::*;
//!
//! let tree = CompiledTree::new(RouteTree::stack(vec![
//!     RouteTree::route("", "home"),
//!     RouteTree::route("detail/:id", "detail"),
//! ]));
//! let registry = RouteRegistry::new()
//!     .route("home",   |_| render! { Home {} })
//!     .route_with("detail", Transition::Slide, |inst| {
//!         render! { Detail(id: inst.params.get("id").cloned().unwrap_or_default()) }
//!     });
//! let handle = RouterHandle::new(tree, registry);
//!
//! render! {
//!     Router(handle: handle.clone()) {
//!         Outlet {}
//!         SwipeBack {}
//!     }
//! }
//! // inside a screen:  use_navigator().navigate(&Target::id("detail"));
//! ```
//!
//! Design lives in [issue #95](https://github.com/whiskerrs/whisker/issues/95).
//!
//! [`docs/router-design.md`]: https://github.com/whiskerrs/whisker/blob/main/docs/router-design.md

#![warn(missing_docs)]

pub mod core;
pub mod render;

// The new API: the pure core graphs/ops + the reactive render layer.
pub use crate::core::{
    CompiledTree, NavError, Navigator, NodeId, NodeInfo, NodePath, RouteDef, RouteInstance,
    RouteState, RouteTree, Scope, StackEntry, StackState, SwitchDef, SwitchState, Target, resolve,
    resolve_within,
};
pub use crate::render::{
    AndroidPredictiveBack, DiagMarker, Layout, Outlet, RenderFn, Role, RouteRegistry, Router,
    RouterHandle, Stack, StackBridge, SwipeBack, Switch, TabBar, TabItem, Tabs, Transition,
    provide_router, use_active_tab, use_navigator,
};
