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

use crate::core::{CompiledTree, NodePath, RouteInstance, RouteTree};
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

    /// Merge `other`'s entries into this one, **keeping existing ids**
    /// (first registration wins). Used by `..spread`: a fragment's id →
    /// render/transition entries fold into the parent registry, and an id
    /// already declared in the parent (or by an earlier spread) is left
    /// untouched — matching the macro's left-to-right declaration order.
    pub fn merge(mut self, other: &RouteRegistry) -> Self {
        for (id, entry) in &other.entries {
            self.entries
                .entry(id.clone())
                .or_insert_with(|| entry.clone());
        }
        self
    }
}

/// A reusable, **un-rooted** group of routes — the value a
/// `routes! { Route(..) Route(..) }` (no single container root) evaluates
/// to. Spread into a `Stack` / `Switch` with `..frag` to splice its routes
/// into that container.
///
/// It carries its route nodes as a list of [`RouteTree`] roots plus the
/// id → render/transition [`RouteRegistry`] entries they need. Spreading the
/// same fragment into N containers creates N tree instances of each route
/// that **dedupe to a single nav target** by id (the registry holds one
/// entry; [`resolve`](crate::core::resolve) picks the right instance
/// relative to the current position).
///
/// `Clone` (cheap — `RouteTree` is a small tree, the registry is
/// `Rc`-backed) so one binding can be spread into several containers.
#[derive(Clone)]
pub struct RouteFragment {
    roots: Vec<RouteTree>,
    registry: RouteRegistry,
}

impl RouteFragment {
    /// Bundle a fragment's route roots with the registry entries they need.
    /// (Emitted by the `routes!` macro; rarely constructed by hand.)
    pub fn new(roots: Vec<RouteTree>, registry: RouteRegistry) -> Self {
        RouteFragment { roots, registry }
    }

    /// The route nodes to splice (cloned at each `..` spread site).
    pub fn roots(&self) -> &[RouteTree] {
        &self.roots
    }

    /// The id → render/transition entries to merge into the parent registry.
    pub fn registry(&self) -> &RouteRegistry {
        &self.registry
    }
}

/// The output of the `routes!` macro: a compiled [`CompiledTree`] paired
/// with its id → component [`RouteRegistry`].
///
/// A `RouteSet` is what a `routes! { … }` declaration evaluates to. The
/// top-level set becomes a [`RouterHandle`](crate::render::RouterHandle)
/// via `RouterHandle::new(routes! { … })`. Hand-built trees + registries
/// convert in with `.into()` (or the tuple form
/// `RouterHandle::new((tree, registry))`), so the macro and the manual path
/// share one constructor.
///
/// (Composable sub-sets — the design's `..content` spread — land with the
/// macro in a later phase; today a `RouteSet` is a single rooted tree.)
pub struct RouteSet {
    pub(crate) tree: CompiledTree,
    pub(crate) registry: RouteRegistry,
    pub(crate) layouts: LayoutRegistry,
}

impl RouteSet {
    /// Bundle a hand-built tree + registry (no layouts) — the manual path.
    pub fn from_parts(tree: CompiledTree, registry: RouteRegistry) -> Self {
        RouteSet {
            tree,
            registry,
            layouts: LayoutRegistry::new(),
        }
    }

    /// Bundle a tree + registry + the `Layout(X)` chrome map — what the
    /// `routes!` macro emits.
    pub fn from_parts_with_layouts(
        tree: CompiledTree,
        registry: RouteRegistry,
        layouts: LayoutRegistry,
    ) -> Self {
        RouteSet {
            tree,
            registry,
            layouts,
        }
    }
}

impl From<(CompiledTree, RouteRegistry)> for RouteSet {
    fn from((tree, registry): (CompiledTree, RouteRegistry)) -> Self {
        RouteSet::from_parts(tree, registry)
    }
}

/// A `Layout(X)` chrome renderer: renders the user's layout component
/// (which draws chrome around an [`Outlet`](crate::render::Outlet)).
#[derive(Clone)]
pub struct LayoutFn(Rc<dyn Fn() -> Element + 'static>);

impl LayoutFn {
    /// Build a [`LayoutFn`] from a closure that renders the layout component.
    pub fn new(f: impl Fn() -> Element + 'static) -> Self {
        LayoutFn(Rc::new(f))
    }

    /// Render the layout component.
    pub fn call(&self) -> Element {
        (self.0)()
    }
}

/// The `routes!`-generated map from a container's [`NodePath`] to the
/// `Layout(X)` component that wraps it. Looked up by `mount_node` when it
/// renders that container, so the chrome (tab bar, drawer, …) is applied
/// from the route tree rather than hand-wired.
#[derive(Clone, Default)]
pub struct LayoutRegistry {
    entries: Vec<(NodePath, LayoutFn)>,
}

impl LayoutRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `layout` as the chrome wrapping the container at `path`.
    /// Chainable.
    pub fn with(mut self, path: NodePath, layout: LayoutFn) -> Self {
        self.entries.push((path, layout));
        self
    }

    /// The layout registered for the container at `path`, if any.
    pub fn get(&self, path: &NodePath) -> Option<&LayoutFn> {
        self.entries.iter().find(|(p, _)| p == path).map(|(_, l)| l)
    }
}
