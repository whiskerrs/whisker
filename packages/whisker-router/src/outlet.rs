//! [`RouteProvider`] + [`Outlet`] + [`router`] — the context-driven
//! foundation that every renderer in this crate stands on.
//!
//! [`RouteProvider`] publishes a [`RouteStack`] into Whisker's context.
//! Descendant components retrieve it with [`router::<R>()`]. Every
//! "real" layout — [`StackLayout`](crate::StackLayout),
//! [`TabsLayout`](crate::TabsLayout), and [`Outlet`] itself — is
//! built on this pattern.
//!
//! [`Outlet`] is the *mount-only* renderer: it observes
//! `stack.current()`, disposes the previous branch on each change,
//! and mounts a fresh one. No transition machinery, no back-stack
//! preservation. Reach for it when you don't need animation; reach
//! for [`StackLayout`](crate::StackLayout) when you do.
//!
//! The pattern follows [`whisker::Show`]: a phantom element acts as
//! the mount slot; an effect observes the reactive route and swaps
//! the previously-mounted branch for a freshly-rendered one.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::runtime::reactive::{effect, Owner};
use whisker::runtime::view::{append_child, create_phantom_element, remove_child, Element};
use whisker::{component, provide_context, use_context, Children};

use crate::route::Route;
use crate::stack::RouteStack;

/// Function prop for [`Outlet`] and [`StackLayout`](crate::StackLayout):
/// maps a route value to its rendered element.
///
/// Wrapped in `Rc` so closures with non-`Copy` captures (e.g. a
/// [`RouteStack`] handle, theme tokens) can be shared between the
/// component's outer body — which `#[component]` re-runs on
/// hot-reload — and the inner mount effect. The [`From`] impl makes
/// `(|r: AppRoute| ...).into()` the usual call-site shape.
///
/// ```ignore
/// let render: RouteRenderFn<AppRoute> = (|r: AppRoute| match r {
///     AppRoute::Home          => render! { Home() },
///     AppRoute::Profile { id } => render! { Profile(id: id) },
/// }).into();
/// ```
#[derive(Clone)]
pub struct RouteRenderFn<R: Route>(pub Rc<dyn Fn(R) -> Element + 'static>);

impl<R: Route> RouteRenderFn<R> {
    /// Invoke the renderer with `route` and return the resulting
    /// element.
    pub fn call(&self, route: R) -> Element {
        (self.0)(route)
    }
}

impl<R, F> From<F> for RouteRenderFn<R>
where
    R: Route,
    F: Fn(R) -> Element + 'static,
{
    fn from(f: F) -> Self {
        RouteRenderFn(Rc::new(f))
    }
}

/// Look up the [`RouteStack`] for route type `R` from context.
///
/// The standard way to drive navigation from inside a screen
/// component without threading the stack handle through every prop:
///
/// ```ignore
/// let nav = router::<AppRoute>();
/// nav.push(AppRoute::Profile { id: 7 });
/// ```
///
/// # Panics
///
/// Panics if no [`RouteProvider`] of that route type is mounted
/// above the caller. Routing is unambiguous at the call site and
/// silently returning `None` would hide the misuse — the panic
/// message names the missing provider type.
pub fn router<R: Route>() -> RouteStack<R> {
    use_context::<RouteStack<R>>()
        .expect("router::<R>() called outside a RouteProvider<R> ancestor")
}

/// Push a [`RouteStack`] into context so the layouts and screens
/// below can look it up via [`router::<R>()`](router).
///
/// Renders nothing of its own — the `children` slot carries the
/// layout (typically [`StackLayout`](crate::StackLayout) or
/// [`TabsLayout`](crate::TabsLayout)) plus everything underneath.
///
/// # Nesting
///
/// Each provider provides one stack type. Nested providers of
/// different `R` coexist — Whisker's type-keyed context lookup picks
/// the nearest ancestor for `R`. That's how the tab-per-stack
/// pattern works: an outer `RouteProvider<TabRoot>` plus one inner
/// `RouteProvider<HomeRoute>` / `RouteProvider<SearchRoute>` per
/// tab, each driving its own `StackLayout`.
///
/// ```ignore
/// render! {
///     RouteProvider(stack: nav.clone()) {
///         StackLayout(render: render.into())
///     }
/// }
/// ```
#[component]
pub fn route_provider<R: Route>(stack: RouteStack<R>, children: Children) -> Element {
    // `#[component]` wraps the body in `FnMut`, so each invocation
    // re-publishes the (cheap, Rc-backed) handle into context.
    provide_context(stack.clone());
    whisker::render! {
        children()
    }
}

/// Mount-only renderer: shows the topmost entry of the in-context
/// [`RouteStack`] via `render`, and nothing else.
///
/// Re-runs the renderer whenever the current route changes. The
/// previously-mounted branch is fully disposed (signals, effects,
/// spawned tasks inside it are dropped) before the new branch
/// mounts — screens don't leak across navigation events, but they
/// also don't survive a back-navigation. If you want a screen's
/// scroll position / form state to come back when the user navigates
/// back, use [`StackLayout`](crate::StackLayout) instead.
///
/// Pulls its [`RouteStack`] from context — wrap a
/// [`RouteProvider`] above it.
///
/// ```ignore
/// render! {
///     RouteProvider(stack: nav) {
///         Outlet(render: (|r: AppRoute| match r {
///             AppRoute::Home          => render! { Home() },
///             AppRoute::Profile { id } => render! { Profile(id: id) },
///         }).into())
///     }
/// }
/// ```
#[component]
pub fn outlet<R: Route>(render: RouteRenderFn<R>) -> Element {
    let stack = router::<R>();
    let frag = create_phantom_element();

    type Mounted = Rc<RefCell<Option<(Owner, Element)>>>;
    let mounted: Mounted = Rc::new(RefCell::new(None));

    let current = stack.current();
    let render = render.clone();

    effect(move || {
        if let Some((owner, handle)) = mounted.borrow_mut().take() {
            remove_child(frag, handle);
            owner.dispose();
        }

        // Mount the new branch under a fresh owner so its signals,
        // effects, and spawned tasks live and die with this entry.
        let route = current.get();
        let owner = Owner::new(None);
        let handle = owner.with(|| {
            let h = render.call(route);
            append_child(frag, h);
            h
        });
        *mounted.borrow_mut() = Some((owner, handle));
    });

    frag
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::RouteError;
    use crate::stack::route_stack;

    #[derive(Clone, Debug, PartialEq)]
    enum TestRoute {
        Home,
        Profile(u64),
    }

    impl Route for TestRoute {
        fn parse(_: &str) -> Result<Self, RouteError> {
            unimplemented!()
        }
        fn to_path(&self) -> String {
            String::new()
        }
    }

    fn with_runtime<F: FnOnce() -> T, T>(f: F) -> T {
        whisker::runtime::reactive::__reset_for_tests();
        let owner = Owner::new(None);
        let out = owner.with(f);
        owner.dispose();
        out
    }

    #[test]
    fn router_lookup_via_context() {
        with_runtime(|| {
            let stack = route_stack(TestRoute::Home);
            provide_context(stack.clone());

            let found = router::<TestRoute>();
            assert_eq!(found.current().get(), TestRoute::Home);

            found.push(TestRoute::Profile(7));
            assert_eq!(stack.current().get(), TestRoute::Profile(7));
        });
    }

    #[test]
    #[should_panic(expected = "outside a RouteProvider<R> ancestor")]
    fn router_lookup_panics_without_context() {
        with_runtime(|| {
            let _ = router::<TestRoute>();
        });
    }

    #[test]
    fn render_fn_wraps_arbitrary_closure() {
        with_runtime(|| {
            let f: RouteRenderFn<TestRoute> = (|r: TestRoute| {
                // The renderer is meant to return an Element; for
                // unit testing we just confirm it's invoked.
                match r {
                    TestRoute::Home => create_phantom_element(),
                    TestRoute::Profile(_) => create_phantom_element(),
                }
            })
            .into();
            let _ = f.call(TestRoute::Home);
        });
    }
}
