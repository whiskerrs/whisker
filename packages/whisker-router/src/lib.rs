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
//! use whisker_router::render::*;
//! use whisker_router::routes;
//!
//! render! {
//!     Router(routes: routes! {
//!         Stack {
//!             Route(path: "", component: Home)
//!             Route(path: "detail/:id", component: Detail)
//!         }
//!     }) {
//!         Outlet {}
//!         SwipeBack {}
//!     }
//! }
//! // inside a screen:  use_navigator().navigate("/detail/42");
//! ```
//!
//! Design lives in [issue #95](https://github.com/whiskerrs/whisker/issues/95).
//!
//! [`docs/router-design.md`]: https://github.com/whiskerrs/whisker/blob/main/docs/router-design.md

#![warn(missing_docs)]

pub mod core;
pub mod plugin;
pub mod render;

pub use crate::plugin::{RouterPlugin, RouterPluginConfig};

/// The declarative route-tree macro — see [`routes`](macro@routes).
pub use whisker_router_macros::routes;

/// Completion markers for the `routes!` macro keywords — **not a public API**.
///
/// The `routes!` macro emits a span-carrying path into this module for each
/// container keyword (`Stack` / `Switch` / `Route` / `Layout`), so rust-analyzer
/// can complete the keyword name while you type — the same trick `render!` uses
/// for built-in tag names via `whisker::__tags`. Has no runtime role.
#[doc(hidden)]
pub mod __kw {
    /// `routes! { Stack { … } }` — ordered container.
    #[derive(Clone, Copy)]
    pub struct Stack;
    /// `routes! { Switch { … } }` — parallel container.
    #[derive(Clone, Copy)]
    pub struct Switch;
    /// `routes! { Route(path: "path", component: Comp) }` — a screen or layout.
    #[derive(Clone, Copy)]
    pub struct Route;
}

// The new API: the pure core graphs/ops + the reactive render layer.
pub use crate::core::{
    CompiledTree, NavError, Navigator, NodeId, NodeInfo, NodePath, RouteDef, RouteInstance,
    RouteState, RouteTree, StackEntry, StackState, SwitchDef, SwitchState, resolve,
};
pub use crate::render::{
    AndroidPredictiveBack, AnimConfig, Direction, Layout, Outlet, Pose, PoseContext, RenderFn,
    Role, RouteFragment, RouteRegistry, RouteSet, RouteTransition, Router, RouterHandle, Stack,
    SwipeBack, Switch, Transition, provide_router, use_navigator, use_param, use_pathname,
};
