//! Provider + Outlet — wire a [`RouteStack`] into context and
//! render the current route through a user-supplied closure.
//!
//! The pattern follows [`whisker::Show`] (control_flow.rs): a
//! phantom element acts as the mount slot; an effect observes
//! `stack.current()` and on each change disposes the previous
//! branch before mounting the new one.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::runtime::reactive::{effect, Owner};
use whisker::runtime::view::{append_child, create_phantom_element, remove_child, Element};
use whisker::{component, provide_context, use_context, Children};

use crate::route::Route;
use crate::stack::RouteStack;

/// Function prop for [`Outlet`]: maps a route value to its rendered
/// element. Wrapped in `Rc` so closures with non-`Copy` captures
/// (e.g. a [`RouteStack`] handle) can be shared between the
/// component's outer remount body and the inner mount effect.
#[derive(Clone)]
pub struct RouteRenderFn<R: Route>(pub Rc<dyn Fn(R) -> Element + 'static>);

impl<R: Route> RouteRenderFn<R> {
    /// Invoke the renderer with a route value.
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
/// Panics if no [`RouteProvider`] of that route type is mounted
/// above the caller — routing is unambiguous at the call site and
/// silently returning `None` would hide the misuse.
pub fn router<R: Route>() -> RouteStack<R> {
    use_context::<RouteStack<R>>()
        .expect("router::<R>() called outside a RouteProvider<R> ancestor")
}

/// `RouteProvider` — push a [`RouteStack`] into context so the
/// layouts and screens below can look it up via [`router`].
///
/// Renders nothing of its own — the `children` slot carries the
/// layout (typically [`StackLayout`](crate::StackLayout) or
/// [`TabsLayout`](crate::TabsLayout)) plus everything underneath.
/// Each provider provides one stack type; nested providers of
/// different `R` coexist (type-keyed lookup picks the nearest
/// ancestor for `R`), which is how tab-per-stack patterns get
/// expressed.
#[component]
pub fn route_provider<R: Route>(stack: RouteStack<R>, children: Children) -> Element {
    // `#[component]` wraps the body in `FnMut` so it can re-run on
    // hot-reload, so the prop is cloned per invocation. `RouteStack`
    // is `Rc`-backed — cheap.
    provide_context(stack.clone());
    whisker::render! {
        children()
    }
}

/// `Outlet` — renders the topmost entry of the in-context
/// [`RouteStack`] via `render`.
///
/// Re-runs the renderer whenever the current route changes. The
/// previously-mounted branch is fully disposed (signals + effects
/// + async tasks inside it are dropped) before the new branch
/// mounts, so screens don't leak across navigation events.
///
/// Pulls its [`RouteStack`] from context; wrap a containing
/// [`RouteProvider`] above it. Useful as the *mount-only* path —
/// no animation machinery; use [`StackLayout`](crate::StackLayout)
/// when you want transitions.
#[component]
pub fn outlet<R: Route>(render: RouteRenderFn<R>) -> Element {
    let stack = router::<R>();
    let frag = create_phantom_element();

    type Mounted = Rc<RefCell<Option<(Owner, Element)>>>;
    let mounted: Mounted = Rc::new(RefCell::new(None));

    let current = stack.current();
    let render = render.clone();

    effect(move || {
        // Tear down the previously-mounted branch (if any).
        if let Some((owner, handle)) = mounted.borrow_mut().take() {
            remove_child(frag, handle);
            owner.dispose();
        }

        // Read the current route and mount its renderer under a
        // fresh owner so all signals/effects/spawned tasks created
        // inside live and die with this entry.
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
