//! # whisker-router
//!
//! Type-safe, signal-backed routing for Whisker.
//!
//! ## The layer cake
//!
//! The crate is built as a small stack of layers — pick the lowest
//! one that does the job and let the rest stay out of your way:
//!
//! 1. [`Route`] — trait that ties a typed enum to its URL form.
//!    Either hand-write `parse` / `to_path`, or annotate the enum with
//!    [`#[route]`](macro@route) and let the macro derive them from per-variant
//!    `#[at("/...")]` patterns.
//! 2. [`RouteStack`] — the cloneable handle holding the back stack.
//!    Created with [`route_stack`]; carries `push` / `back` / `replace`
//!    plus reactive readers ([`RouteStack::current`],
//!    [`RouteStack::depth`], [`RouteStack::can_back`]).
//! 3. [`RouteProvider`] — context provider that publishes the stack
//!    so descendants can look it up via [`router::<R>()`](router).
//!    One provider per route type; nested providers of different `R`
//!    coexist (this is how tab-per-stack patterns are built).
//! 4. **Renderers** — pick the one that fits the surface area:
//!    - [`Outlet`] — mount-only, no animation; the cheapest path.
//!    - [`StackLayout`] — back-stack-preserving stack navigator with
//!      pluggable animation. The canonical choice for screen stacks.
//!    - [`TabsLayout`] — keep-alive tab switcher driven by the same
//!      stack via per-tab predicates.
//!    - [`ModalLayout`] — slide-from-bottom modal sheet.
//!    - [`Pane`] — display-toggleable container; a building block
//!      for custom layouts (used internally by [`TabsLayout`]).
//! 5. [`StackTransition`] — pluggable animation interface used by
//!    [`StackLayout`]. Built-ins: [`IosSlide`] (default), [`Fade`],
//!    [`VerticalSlide`], [`Instant`].
//! 6. **Gesture components** — opt-in interactivity, mounted as
//!    children of [`StackLayout`]: [`IosSwipeBack`] for the iOS edge
//!    swipe, [`AndroidPredictiveBack`] for the Android system back.
//! 7. [`on_back`] — LIFO back-handler chain for ad-hoc consumers
//!    (modals, search bars) that want to intercept a back press.
//! 8. [`linking`] — minimal deep-link surface: [`linking::initial_url`]
//!    + [`linking::on_url`].
//!
//! ## Design notes
//!
//! All routing logic runs inside a single [`whisker::runtime`]
//! instance — there is intentionally no per-screen `UIViewController`
//! / `Fragment`. Gestures, transitions, and freeze are implemented
//! entirely on the Whisker side for cross-platform parity.
//!
//! Design lives in [issue #95](https://github.com/whiskerrs/whisker/issues/95).
//!
//! ## Minimal usage
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_router::{route, route_stack, RouteProvider, StackLayout};
//!
//! #[route]
//! #[derive(Clone, Debug, PartialEq)]
//! pub enum AppRoute {
//!     #[at("/")]            Home,
//!     #[at("/profile/:id")] Profile { id: u64 },
//! }
//!
//! #[component]
//! fn app() -> Element {
//!     let nav = route_stack(AppRoute::Home);
//!     render! {
//!         RouteProvider(stack: nav.clone()) {
//!             StackLayout(render: (|r: AppRoute| match r {
//!                 AppRoute::Home          => render! { Home() },
//!                 AppRoute::Profile { id } => render! { Profile(id: id) },
//!             }).into())
//!         }
//!     }
//! }
//! ```

#![warn(missing_docs)]

pub mod back_handler;
pub mod core;
pub mod gestures;
pub mod layouts;
pub mod linking;
pub mod outlet;
pub mod route;
pub mod stack;
pub mod transitions;

/// `#[route]` attribute macro — generates a [`Route`] impl from a
/// per-variant `#[at("/...")]` pattern.
///
/// `:foo` segments bind to named fields of the same name; literal
/// segments match verbatim. Field types must implement `FromStr`
/// (for `parse`) and `Display` (for `to_path`). Tuple-struct variants
/// aren't supported in v1 — switch to a named-field form.
///
/// See [`whisker_router_macros`] for the full grammar.
///
/// ```ignore
/// use whisker_router::route;
///
/// #[route]
/// #[derive(Clone, Debug, PartialEq)]
/// pub enum AppRoute {
///     #[at("/")]                Home,
///     #[at("/profile/:id")]     Profile { id: u64 },
///     #[at("/settings")]        Settings,
/// }
/// ```
pub use whisker_router_macros::route;

pub use crate::back_handler::{BackHandlerGuard, on_back};
pub use crate::gestures::{
    AndroidPredictiveBack, AndroidPredictiveBackProps, IosSwipeBack, IosSwipeBackProps,
};
pub use crate::layouts::modal::{ModalLayout, ModalLayoutProps, ModalRenderFn};
pub use crate::layouts::pane::{Pane, PaneProps};
pub use crate::layouts::stack::{StackLayout, StackLayoutHandle, StackLayoutProps};
pub use crate::layouts::tabs::{TabSpec, TabsLayout, TabsLayoutProps};
pub use crate::outlet::{
    Outlet, OutletProps, RouteProvider, RouteProviderProps, RouteRenderFn, router,
};
pub use crate::route::{Route, RouteError};
pub use crate::stack::{EntryId, EntryState, RouteEntry, RouteStack, route_stack};
pub use crate::transitions::{
    Direction, Fade, IOS_PARALLAX_PCT, Instant, IosSlide, Side, StackTransition,
    StackTransitionBox, VerticalSlide,
};
