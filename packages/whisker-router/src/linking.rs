//! Deep-link surface — cold-launch URL + post-launch URL subscription.
//!
//! Two functions, in the shape every major mobile framework uses:
//!
//! - [`initial_url`] — synchronous read of the URL the app was
//!   cold-launched with, if any. Used to seed the
//!   [`RouteStack`](crate::RouteStack) at boot.
//! - [`on_url`] — subscription for URLs delivered after launch
//!   (e.g. a deep-link tap into an already-running app).
//!
//! ```ignore
//! use whisker_router::{linking, route_stack, Route};
//!
//! let initial = linking::initial_url()
//!     .and_then(|u| AppRoute::parse(&u).ok())
//!     .unwrap_or(AppRoute::Home);
//! let nav = route_stack(initial);
//!
//! linking::on_url(move |url| {
//!     if let Ok(r) = AppRoute::parse(&url) {
//!         nav.push(r);
//!     }
//! });
//! ```
//!
//! # Status
//!
//! Both functions are stubs in the current revision — wiring through
//! the `whisker::module!` mechanism is tracked as part of issue #95
//! (the runtime-side `on_global_event` primitive needs to ship
//! first). The API surface here is what callers should code against
//! in the meantime.

#![allow(unused_variables)]

/// Returns the URL the app was cold-launched with, if any.
///
/// Stubbed until the `WhiskerLinking` Lynx module is wired up.
/// Always returns `None` today.
pub fn initial_url() -> Option<String> {
    None
}

/// Subscribe to URLs delivered after launch.
///
/// `handler` is invoked once per delivered URL on the runtime
/// thread. Stubbed until the global-event API is exposed from the
/// runtime — `handler` is currently never invoked.
pub fn on_url<F>(_handler: F)
where
    F: Fn(String) + 'static,
{
}
