//! Routing surface for the podcast example.
//!
//! Both [`AppRoute`] and [`Navigator`] live here (and not in the
//! top-level `podcast` crate) so the feature crates can read them
//! out of context without re-defining structurally-identical
//! copies — `use_context` matches on the `TypeId`, and two `Rust`
//! types that share a layout but live in different crates have
//! different `TypeId`s.

use std::rc::Rc;

use whisker_router::route;

/// Where to go next. URL form is set up so a future deep-link
/// (`/podcast/12345`) parses cleanly through `whisker-router`; the
/// in-app navigation just hands the typed enum to the stack.
#[route]
#[derive(Clone, Debug, PartialEq)]
pub enum AppRoute {
    #[at("/")]
    Browse,
    #[at("/podcast/:id")]
    Detail { id: u64 },
    #[at("/search")]
    Search,
}

/// Navigation surface the feature crates see. Both methods are
/// thin closures over the underlying [`whisker_router::RouteStack`]
/// the top-level shell creates — keeps Browse / Detail off
/// `whisker-router` entirely and lets the shell swap routers later
/// without touching the features.
///
/// Cloning the struct is one [`Rc::clone`] per field; the wrapped
/// closures hold a clone of the same `RouteStack` handle.
#[derive(Clone)]
pub struct Navigator {
    pub show_detail: Rc<dyn Fn(u64)>,
    pub show_search: Rc<dyn Fn()>,
    pub go_back: Rc<dyn Fn()>,
}
