//! `TabsLayout` — keep-alive tab switcher driven by a single
//! [`RouteStack`].
//!
//! Each tab is a `(matches, content)` pair: `matches` decides which
//! current route belongs to this tab, `content` renders that tab's
//! subtree. All tabs are mounted simultaneously and toggled between
//! `display: flex` / `display: none` so state (scroll position,
//! in-flight requests, signal values) survives switching — the same
//! technique React Native's `BottomTabNavigator` and SwiftUI's
//! `TabView` use.
//!
//! The expo-router-style nested-route design lives one level up: a
//! parent enum's `#[layout(TabsLayout)]` variants each correspond to
//! one tab, and the per-tab subtree is itself a `StackLayout` over
//! that variant's inner route enum. From the `RouteStack`'s point of
//! view the global stack is still a single sequence; tabs are pure
//! projection.
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_router::{route_stack, TabSpec, TabsLayout};
//!
//! let nav = route_stack(AppRoute::Home(HomeRoute::Index));
//!
//! render! {
//!     TabsLayout(
//!         nav: nav.clone(),
//!         tabs: vec![
//!             TabSpec::new(
//!                 |r: &AppRoute| matches!(r, AppRoute::Home(_)),
//!                 || render! { HomeStack() },
//!             ),
//!             TabSpec::new(
//!                 |r: &AppRoute| matches!(r, AppRoute::Search(_)),
//!                 || render! { SearchStack() },
//!             ),
//!         ],
//!         bar: BottomBar(nav: nav.clone()).into(),
//!     )
//! }
//! ```

use std::rc::Rc;

use whisker::css::ext::*;
use whisker::css::{Css, Display, FlexDirection, ToCss};
use whisker::runtime::element::ElementTag;
use whisker::runtime::view::apply::apply_styles;
use whisker::runtime::view::{append_child, create_element, Element};
use whisker::{component, computed};

use crate::outlet::router;
use crate::route::Route;

/// One tab in a [`TabsLayout`] — a predicate over the current route
/// plus the renderer for that tab's body.
///
/// Both closures are held in `Rc` so the struct is `Clone` (the
/// `#[component]` macro re-clones every prop on each `FnMut` body
/// invocation). `matches` is invoked inside a reactive `computed`
/// every time the current route changes, so keep it cheap — usually
/// a `matches!()` against an enum variant.
pub struct TabSpec<R: Route> {
    matches: Rc<dyn Fn(&R) -> bool + 'static>,
    content: Rc<dyn Fn() -> Element + 'static>,
}

impl<R: Route> Clone for TabSpec<R> {
    fn clone(&self) -> Self {
        TabSpec {
            matches: Rc::clone(&self.matches),
            content: Rc::clone(&self.content),
        }
    }
}

impl<R: Route> TabSpec<R> {
    /// Build a tab from a predicate and a body renderer.
    pub fn new<M, C>(matches: M, content: C) -> Self
    where
        M: Fn(&R) -> bool + 'static,
        C: Fn() -> Element + 'static,
    {
        TabSpec {
            matches: Rc::new(matches),
            content: Rc::new(content),
        }
    }
}

/// Keep-alive tab switcher.
///
/// Renders each tab's body inside its own pane; only the pane whose
/// `matches` predicate accepts the current route is set to
/// `display: flex`, the rest are `display: none`. An optional `bar`
/// element (typically a bottom tab bar) is appended below the panes
/// in column order.
///
/// The [`RouteStack`](crate::RouteStack) for route type `R` is
/// pulled from context — wrap a [`RouteProvider`](crate::RouteProvider)
/// above the layout so `router::<R>()` finds it.
///
/// Built directly against the runtime API rather than via `render!`
/// for two reasons: (1) the macro can't take an arbitrary
/// `Vec<TabSpec>` and unroll it, and (2) the bar is an already-built
/// `Element` value which the macro doesn't accept as a child slot.
#[component]
pub fn tabs_layout<R: Route>(
    tabs: Vec<TabSpec<R>>,
    #[prop(default = None)] bar: Option<Element>,
) -> Element {
    let stack = router::<R>();
    let container = create_element(ElementTag::View);
    apply_styles(container, container_css().to_css_string());

    // Pane area — grows to fill the space above the bar.
    let pane_area = create_element(ElementTag::View);
    apply_styles(pane_area, pane_area_css().to_css_string());
    append_child(container, pane_area);

    let current = stack.current();
    // Clone the tab list out of the FnMut capture so we can consume
    // it. `TabSpec` is cheap to clone (two `Rc` bumps each).
    for tab in tabs.clone().into_iter() {
        let pane = create_element(ElementTag::View);
        let matches = tab.matches.clone();
        let current_for_pane = current;
        let style = computed(move || {
            let on = matches(&current_for_pane.get());
            pane_css(on).to_css_string()
        });
        apply_styles::<_, String>(pane, style);

        let body = (tab.content)();
        append_child(pane, body);
        append_child(pane_area, pane);
    }

    if let Some(bar_el) = bar {
        append_child(container, bar_el);
    }

    container
}

fn container_css() -> Css {
    Css::new()
        .flex_direction(FlexDirection::Column)
        .width(100.percent())
        .height(100.percent())
}

fn pane_area_css() -> Css {
    // `flex: 1` equivalent — grow to consume vertical space left
    // over by the bar.
    Css::new()
        .flex_grow(1.0)
        .flex_direction(FlexDirection::Column)
        .width(100.percent())
}

fn pane_css(visible: bool) -> Css {
    // Each pane covers the full pane area; only one is visible at a
    // time. `position: absolute` would also work but `display: none`
    // + flex-grow lets layout fall through naturally to the bar.
    let base = Css::new()
        .flex_direction(FlexDirection::Column)
        .width(100.percent())
        .height(100.percent());
    if visible {
        base.display(Display::Flex)
    } else {
        base.display(Display::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::RouteError;

    #[derive(Clone, Debug, PartialEq)]
    enum TestRoute {
        Home,
        Search,
    }

    impl Route for TestRoute {
        fn parse(_: &str) -> Result<Self, RouteError> {
            unimplemented!()
        }
        fn to_path(&self) -> String {
            String::new()
        }
    }

    #[test]
    fn tab_spec_predicate_runs() {
        let spec: TabSpec<TestRoute> = TabSpec::new(
            |r| matches!(r, TestRoute::Home),
            whisker::runtime::view::create_phantom_element,
        );
        assert!((spec.matches)(&TestRoute::Home));
        assert!(!(spec.matches)(&TestRoute::Search));
    }

    #[test]
    fn tab_spec_is_clone() {
        // Static check: `#[component]` requires Clone for FnMut
        // captures. If `TabSpec` ever stops being Clone, this fails
        // at compile time before showing up as a downstream macro
        // error.
        fn assert_clone<T: Clone>() {}
        assert_clone::<TabSpec<TestRoute>>();
    }

    #[test]
    fn pane_css_visible_emits_flex() {
        let css = pane_css(true).to_css_string();
        assert!(css.contains("display: flex"), "got {css}");
    }

    #[test]
    fn pane_css_hidden_emits_none() {
        let css = pane_css(false).to_css_string();
        assert!(css.contains("display: none"), "got {css}");
    }

    #[test]
    fn container_uses_column_layout() {
        let css = container_css().to_css_string();
        assert!(css.contains("flex-direction: column"), "got {css}");
    }

    #[test]
    fn pane_area_grows() {
        let css = pane_area_css().to_css_string();
        assert!(css.contains("flex-grow: 1"), "got {css}");
    }
}
