//! # Reactive rendering layer
//!
//! This module draws the [`RouteTree`](crate::core::RouteTree) /
//! [`RouteState`](crate::core::RouteState) core in the Whisker reactive
//! runtime: a signal-backed [`RouterHandle`] + [`use_navigator`], the
//! [`Outlet`] / [`Stack`] / [`Switch`] renderers,
//! float-`Tween` transitions (via `whisker-animation`, **not** Lynx's
//! animator), and the interactive back gestures ([`SwipeBack`] for iOS
//! edge swipe, [`AndroidPredictiveBack`] for Android 13+ predictive back)
//! — both driving the same coordinated two-screen scrub.
//!
//! The id → component map is built by the [`routes!`](crate::routes) macro
//! (which also builds the route tree); for advanced use-cases a
//! [`RouteRegistry`] can be assembled by hand.
//!
//! ## The signal model
//!
//! [`RouterHandle`] owns the immutable [`CompiledTree`](crate::core::CompiledTree),
//! the [`RouteRegistry`], and a single `RwSignal<RouteState>`. Every verb
//! (`navigate` / `select` / `back` / `replace` / `pop_to` / `reset`)
//! clones the state, runs the [`Navigator`](crate::core::Navigator) op,
//! and writes the signal back. `current` is a `computed`, never stored.
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
//! ## Usage
//!
//! ```ignore
//! use whisker_router::{routes, render::*};
//!
//! let handle = RouterHandle::new(routes! {
//!     Stack {
//!         Route(path: "", component: Home)
//!         Route(path: "detail/:id", component: Detail)
//!     }
//! });
//!
//! render! {
//!     Router(handle: handle) {
//!         Outlet {}
//!         SwipeBack {}
//!     }
//! }
//! // inside a screen:  use_navigator().navigate("/detail/42");
//! ```

pub mod components;
pub mod gesture;
pub mod handle;
pub mod node;
pub mod registry;
pub mod transition;

#[cfg(test)]
mod tests;

pub use components::{
    Layout, LayoutProps, OutletAnchor, Router, RouterProps, RouterRoot, Stack, StackProps, Switch,
    SwitchProps,
};
pub use components::{Outlet, OutletProps};
pub use gesture::{AndroidPredictiveBack, AndroidPredictiveBackProps, SwipeBack, SwipeBackProps};
pub use handle::{RouterHandle, provide_router, use_navigator, use_param, use_pathname};
pub use registry::{LayoutFn, LayoutRegistry, RenderFn, RouteFragment, RouteRegistry, RouteSet};
pub use transition::{Direction, Pose, PoseContext, Role, RouteTransition, Transition};
// Re-exported for custom `Transition` authors (the return type of
// `config()`), so everything needed for a custom transition is in one place.
pub use whisker::AnimConfig;
