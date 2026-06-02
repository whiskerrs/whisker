//! # whisker-router
//!
//! Type-safe, signal-backed routing for Whisker.
//!
//! Design lives in [issue #95](https://github.com/whiskerrs/whisker/issues/95).
//! The crate ships:
//!
//! - [`Route`] — trait that ties a typed enum to its URL form.
//! - [`RouteStack`] — first-class signal-backed back stack.
//! - [`RouteProvider`] — pushes a [`RouteStack`] into context so
//!   the layouts (and screens) below can resolve it with
//!   [`router::<R>()`](router).
//! - Layouts in [`layouts`] — [`StackLayout`], [`TabsLayout`],
//!   [`ModalLayout`], [`Pane`]. Each layout pulls its stack from
//!   the nearest ancestor [`RouteProvider`].
//! - [`Outlet`] — mount-only variant of [`StackLayout`] (no
//!   transition machinery), also context-driven.
//! - [`on_back`] — LIFO back-handler chain.
//! - [`linking`] — minimal deep-link surface: `initial_url` +
//!   `on_url`.
//!
//! All routing logic runs inside a single [`whisker::runtime`]
//! instance — there is intentionally no per-screen
//! `UIViewController` / `Fragment`. Gestures, transitions, and
//! freeze are implemented entirely on the Whisker side for
//! cross-platform parity.

#![warn(missing_docs)]

pub mod back_handler;
pub mod gestures;
pub mod layouts;
pub mod linking;
pub mod outlet;
pub mod route;
pub mod stack;
pub mod transitions;

/// `#[route]` attribute macro — generates a `Route` impl from a
/// per-variant `#[at("/...")]` pattern.
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

pub use crate::back_handler::{on_back, BackHandlerGuard};
pub use crate::gestures::{IosSwipeBack, IosSwipeBackProps};
pub use crate::layouts::modal::{ModalLayout, ModalLayoutProps, ModalRenderFn};
pub use crate::layouts::pane::{Pane, PaneProps};
pub use crate::layouts::stack::{StackLayout, StackLayoutHandle, StackLayoutProps};
pub use crate::layouts::tabs::{TabSpec, TabsLayout, TabsLayoutProps};
pub use crate::outlet::{
    router, Outlet, OutletProps, RouteProvider, RouteProviderProps, RouteRenderFn,
};
pub use crate::route::{Route, RouteError};
pub use crate::stack::{route_stack, EntryId, EntryState, RouteEntry, RouteStack};
pub use crate::transitions::{
    Direction, Fade, Instant, IosSlide, Side, StackTransition, StackTransitionBox, VerticalSlide,
    IOS_PARALLAX_PCT,
};
