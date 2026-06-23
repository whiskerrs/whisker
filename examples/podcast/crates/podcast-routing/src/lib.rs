//! Routing surface for the podcast example: the [`Navigator`] facade.
//!
//! Only [`Navigator`] lives here (and not in the top-level `podcast` crate) so
//! the feature crates can read it out of context without re-defining a
//! structurally-identical copy — `use_context` matches on the `TypeId`, and two
//! Rust types that share a layout but live in different crates have different
//! `TypeId`s.
//!
//! The route *tree* itself now lives in the shell's `routes! { … }` (the new
//! `whisker-router` macro); this crate only carries the thin navigation facade
//! the features call. The shell wires each closure to the underlying
//! [`whisker_router::RouterHandle`] (URL navigation: `/`, `/podcast/:id`,
//! `/search`), so Browse / Detail / Search stay unaware of the router entirely.

use std::rc::Rc;

/// Navigation surface the feature crates see. Each field is a thin closure the
/// shell wires to the underlying [`whisker_router::RouterHandle`] — keeps
/// Browse / Detail / Search off `whisker-router` and lets the shell swap the
/// routing layer without touching the features.
///
/// Cloning the struct is one [`Rc::clone`] per field; the wrapped closures
/// share one clone of the same router handle.
#[derive(Clone)]
pub struct Navigator {
    /// Push the detail screen for the podcast with this iTunes `collectionId`
    /// (`navigate("/podcast/:id")`).
    pub show_detail: Rc<dyn Fn(u64)>,
    /// Push the search screen (`navigate("/search")`).
    pub show_search: Rc<dyn Fn()>,
    /// Pop the top of the stack (`back()`).
    pub go_back: Rc<dyn Fn()>,
}
