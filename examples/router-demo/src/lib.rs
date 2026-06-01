//! Router demo — exercises the `whisker-router` public API end-to-end.
//!
//! Routes are declared with the `#[route]` attribute macro. The
//! `render_app` entry creates a [`RouteStack`] and pushes it into
//! context via [`RouteProvider`]; [`StackLayout`] then renders the
//! current entry with iOS-style slide animations. Screens reach the
//! stack through `router::<AppRoute>()` and call `push` / `back` from
//! event handlers.
//!
//! The integration test in `tests/navigates_through_screens.rs`
//! installs a recording renderer and walks the app through home →
//! list → post(7) → back → settings.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::{
    route, route_stack, router, RouteProvider, RouteProviderProps, RouteRenderFn, RouteStack,
    StackLayout, StackLayoutProps,
};

/// Top-level route enum.
///
/// `#[route]` reads each variant's `#[at("…")]` pattern and
/// generates a `Route` impl. The derived `Clone + Debug + PartialEq`
/// are required by the trait bound on `RouteStack`.
#[route]
#[derive(Clone, Debug, PartialEq)]
pub enum AppRoute {
    #[at("/")]
    Home,
    #[at("/list")]
    List,
    #[at("/post/:id")]
    Post { id: u64 },
    #[at("/settings")]
    Settings,
}

/// Layout shared by every screen — column with padded buttons.
/// The background colour is parameterised so each route renders in
/// a distinct hue, making it easy to see which screen sits on top
/// (vs. underneath) during a swipe-back / push animation.
fn screen_style(bg: &str) -> String {
    format!(
        "display: flex; flex-direction: column; gap: 8px; padding: 16px; \
         width: 100%; height: 100%; background-color: {bg}; overflow: visible;"
    )
}

fn link_style() -> &'static str {
    "padding: 6px 12px; background: rgba(0, 0, 0, 0.08); border-radius: 4px;"
}

/// Home screen — two `push` actions to seed other test paths.
#[component]
pub fn home_screen() -> Element {
    let nav = router::<AppRoute>();
    let nav_list = nav.clone();
    let nav_settings = nav.clone();
    render! {
        view(style: screen_style("#fef3c7")) {
            text(style: "font-size: 24px;") { text(value: "Home") }
            view(
                style: link_style(),
                on_tap: move |_| nav_list.push(AppRoute::List),
            ) {
                text(value: "Open list")
            }
            view(
                style: link_style(),
                on_tap: move |_| nav_settings.push(AppRoute::Settings),
            ) {
                text(value: "Open settings")
            }
        }
    }
}

/// List screen — pushes a `Post { id }` to demonstrate a route with
/// a parameter.
#[component]
pub fn list_screen() -> Element {
    let nav = router::<AppRoute>();
    let nav_post = nav.clone();
    let nav_back = nav.clone();
    render! {
        view(style: screen_style("#d1fae5")) {
            text(style: "font-size: 24px;") { text(value: "List") }
            view(
                style: link_style(),
                on_tap: move |_| nav_post.push(AppRoute::Post { id: 7 }),
            ) {
                text(value: "Open post 7")
            }
            view(
                style: link_style(),
                on_tap: move |_| { nav_back.back(); },
            ) {
                text(value: "Back")
            }
        }
    }
}

/// Post screen — reads its `id` prop, demonstrates `back()`.
#[component]
pub fn post_screen(id: u64) -> Element {
    let nav = router::<AppRoute>();
    let label = computed(move || format!("Post #{id}"));
    render! {
        view(style: screen_style("#dbeafe")) {
            text(style: "font-size: 24px;") { text(value: label) }
            view(
                style: link_style(),
                on_tap: move |_| { nav.back(); },
            ) {
                text(value: "Back")
            }
        }
    }
}

/// Settings screen — demonstrates `replace_all` (jump back to the
/// root of the stack).
#[component]
pub fn settings_screen() -> Element {
    let nav = router::<AppRoute>();
    render! {
        view(style: screen_style("#fce7f3")) {
            text(style: "font-size: 24px;") { text(value: "Settings") }
            view(
                style: link_style(),
                on_tap: move |_| nav.replace_all(AppRoute::Home),
            ) {
                text(value: "Reset to home")
            }
        }
    }
}

/// Build the app rooted at `stack`.
///
/// Exposed (rather than baked into `render_app`) so the integration
/// test can hold the same `RouteStack` handle the UI uses, and drive
/// it via `push` / `back` between renders.
pub fn render_with(stack: RouteStack<AppRoute>) -> Element {
    let render: RouteRenderFn<AppRoute> = (|r: AppRoute| match r {
        AppRoute::Home => render! { HomeScreen() },
        AppRoute::List => render! { ListScreen() },
        AppRoute::Post { id } => render! { PostScreen(id: id) },
        AppRoute::Settings => render! { SettingsScreen() },
    })
    .into();
    render! {
        RouteProvider(stack: stack) {
            StackLayout(render: render.clone())
        }
    }
}

/// Production entry — fresh `RouteStack` rooted at `AppRoute::Home`.
///
/// Wraps everything in a viewport-sized `page` so the in-app content
/// has a width/height to lay out against on iOS/Android. Background
/// is white; each screen draws its own padded content on top.
#[whisker::main]
pub fn render_app() -> Element {
    let stack = route_stack(AppRoute::Home);
    let render: RouteRenderFn<AppRoute> = (|r: AppRoute| match r {
        AppRoute::Home => render! { HomeScreen() },
        AppRoute::List => render! { ListScreen() },
        AppRoute::Post { id } => render! { PostScreen(id: id) },
        AppRoute::Settings => render! { SettingsScreen() },
    })
    .into();
    render! {
        page(
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column;",
        ) {
            RouteProvider(stack: stack) {
                StackLayout(render: render.clone())
            }
        }
    }
}
