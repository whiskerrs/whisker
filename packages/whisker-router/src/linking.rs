//! Deep-link surface — Level 1 only.
//!
//! ```ignore
//! use whisker_router::linking;
//!
//! let initial = linking::initial_url()
//!     .and_then(|u| AppRoute::parse(&u).ok())
//!     .unwrap_or(AppRoute::default_root());
//!
//! linking::on_url(move |url| {
//!     // user decides routing / fallback
//! });
//! ```
//!
//! Wiring through the `whisker::module!` mechanism is implemented
//! in a follow-up — this module is currently a documentation +
//! API-shape placeholder while the runtime-side `on_global_event`
//! primitive is confirmed (issue #95 deps).

#![allow(unused_variables)]

/// Returns the URL the app was cold-launched with, if any.
///
/// Stubbed until the `WhiskerLinking` Lynx module is wired up.
pub fn initial_url() -> Option<String> {
    None
}

/// Subscribe to URLs delivered after launch.
///
/// Stubbed until the global-event API is exposed from the runtime
/// — tracked as part of the cross-crate dependency audit.
pub fn on_url<F>(_handler: F)
where
    F: Fn(String) + 'static,
{
}
