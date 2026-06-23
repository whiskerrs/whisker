//! The hand-written **route id → render fn** registry, plus the
//! per-route [`RouteTransition`] choice.
//!
//! In a later phase the `routes!` macro will *generate* this map from
//! the declared screens. For now it is built by hand: the app registers
//! a render closure for every [`RouteDef::id`](crate::core::RouteDef::id)
//! in its [`CompiledTree`], optionally with a transition, and the
//! [`RouterHandle`](crate::render::RouterHandle) looks the closure up
//! when an `Outlet` needs to mount a leaf.
//!
//! ```ignore
//! let registry = RouteRegistry::new()
//!     .route("home", |_| render! { Home {} })
//!     .route_with("detail", RouteTransition::slide(), |inst| {
//!         let id = inst.params.get("id").cloned().unwrap_or_default();
//!         render! { Detail(id: id) }
//!     });
//! ```

use std::collections::HashMap;
use std::rc::Rc;

use whisker::runtime::view::Element;

use crate::core::RouteInstance;
use crate::render::transition::RouteTransition;

/// A screen-render closure: maps the concrete [`RouteInstance`] (its
/// param values) to a freshly-rendered [`Element`].
///
/// Wrapped in `Rc` so it can be shared between the registry and the
/// per-mount effect inside an `Outlet` (which re-runs on every swap).
#[derive(Clone)]
pub struct RenderFn(pub Rc<dyn Fn(&RouteInstance) -> Element + 'static>);

impl RenderFn {
    /// Build a [`RenderFn`] from any closure.
    pub fn new(f: impl Fn(&RouteInstance) -> Element + 'static) -> Self {
        RenderFn(Rc::new(f))
    }

    /// Invoke the renderer for `instance`.
    pub fn call(&self, instance: &RouteInstance) -> Element {
        (self.0)(instance)
    }
}

impl<F> From<F> for RenderFn
where
    F: Fn(&RouteInstance) -> Element + 'static,
{
    fn from(f: F) -> Self {
        RenderFn::new(f)
    }
}

/// One registered route: its render closure + transition.
#[derive(Clone)]
struct Entry {
    render: RenderFn,
    transition: RouteTransition,
}

/// The hand-written map from a route id to its [`RenderFn`] +
/// [`RouteTransition`].
///
/// Cloneable (cheap — the closures are `Rc`-backed) so it can be moved
/// into a [`RouterHandle`](crate::render::RouterHandle) and shared.
#[derive(Clone, Default)]
pub struct RouteRegistry {
    entries: HashMap<String, Entry>,
}

impl RouteRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `id`'s render closure with the platform-default
    /// ([`RouteTransition::default`]) transition. Chainable.
    pub fn route(mut self, id: impl Into<String>, render: impl Into<RenderFn>) -> Self {
        self.insert(id.into(), render.into(), RouteTransition::default());
        self
    }

    /// Register `id`'s render closure with an explicit `transition`.
    /// Chainable.
    pub fn route_with(
        mut self,
        id: impl Into<String>,
        transition: RouteTransition,
        render: impl Into<RenderFn>,
    ) -> Self {
        self.insert(id.into(), render.into(), transition);
        self
    }

    fn insert(&mut self, id: String, render: RenderFn, transition: RouteTransition) {
        self.entries.insert(id, Entry { render, transition });
    }

    /// The render closure registered for `id`, if any.
    pub fn render_fn(&self, id: &str) -> Option<RenderFn> {
        self.entries.get(id).map(|e| e.render.clone())
    }

    /// The transition registered for `id` (defaults to the
    /// platform [`RouteTransition::default`] when the id is unknown — the safe
    /// fallback for a missing registration).
    pub fn transition(&self, id: &str) -> RouteTransition {
        self.entries
            .get(id)
            .map(|e| e.transition.clone())
            .unwrap_or_default()
    }

    /// Whether `id` has a registered render closure.
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
}
