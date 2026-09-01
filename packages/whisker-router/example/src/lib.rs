//! Example app for `whisker-router` — Route nesting with tabs:
//!
//! ```text
//! Route(component: TabsLayout) {          // layout: tab bar chrome + Outlet
//!     Switch {
//!         Route(path: "(home)") {          // group: no URL segment
//!             Stack {
//!                 Route(path: "", component: Home)
//!                 Route(path: "detail/:id", component: Detail)
//!             }
//!         }
//!         Route(path: "(search)") {
//!             Stack {
//!                 Route(path: "list", component: ListScreen)
//!                 Route(path: "detail/:id", component: Detail)
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! - **Route nesting**: `Route(component: TabsLayout)` at the root is a layout
//!   route — its component renders with an `Outlet` for children, like
//!   expo-router's `_layout.tsx`.
//! - **Group routes**: `Route(path: "(home)")` and `Route(path: "(search)")`
//!   are pathless groups (expo-router's `(group)` folders). They don't add a
//!   URL segment but organize children under a Switch branch.
//! - **Custom tab bar**: built with `use_group` + `navigator.navigate("/(home)")` —
//!   no built-in TabBar component needed.
//! - **Back gestures**: `SwipeBack` (iOS) and `AndroidPredictiveBack` (Android 13+).

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::render::{
    AndroidPredictiveBack, Outlet, Router, RouterHandle, SwipeBack, use_group, use_navigator,
    use_param,
};
use whisker_router::{ReselectBehavior, routes};

/// Tab bar layout: an `Outlet` for the active branch above a custom tab bar.
#[component]
fn tabs_layout() -> Element {
    render! {
        View(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            View(style: css!(
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                Outlet {}
            }
            MyTabBar {}
        }
    }
}

#[component]
fn my_tab_bar() -> Element {
    let nav = use_navigator();
    let active_group = use_group();

    render! {
        View(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceAround,
            align_items: AlignItems::Center,
            height: px(56),
            background_color: Color::hex(0x16161D),
        )) {
            TabBarItem(
                label: "Home",
                group: "home",
                url: "/(home)",
                active_group: active_group,
                nav: nav.clone(),
            )
            TabBarItem(
                label: "List",
                group: "search",
                url: "/(search)",
                active_group: active_group,
                nav: nav.clone(),
            )
        }
    }
}

#[component]
fn tab_bar_item(
    label: &'static str,
    group: &'static str,
    url: &'static str,
    active_group: ReadSignal<Option<String>>,
    nav: RouterHandle,
) -> Element {
    let is_active = computed(move || active_group.get().as_deref() == Some(group));
    let nav = nav.clone();
    render! {
        View(
            style: computed(move || {
                let opacity = if is_active.get() { 1.0 } else { 0.5 };
                css!(
                    flex_grow: 1.0,
                    display: Display::Flex,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    height: px(56),
                    opacity: opacity,
                )
            }),
            on_tap: move |_| {
                let _ = nav.navigate(url);
            },
        ) {
            Text(
                value: label.to_string(),
                style: css!(color: Color::hex(0xFFFFFF), font_size: px(13)),
            )
        }
    }
}

#[whisker::main]
fn app() -> Element {
    render! {
        Router(routes: routes! {
            Route(component: TabsLayout) {
                Switch(
                    initial: "(home)",
                    reselect: ReselectBehavior::PopToRoot,
                ) {
                    Route(path: "(home)") {
                        Stack {
                            Route(path: "", component: Home)
                            Route(path: "detail/:id", component: Detail)
                        }
                    }
                    Route(path: "(search)") {
                        Stack {
                            Route(path: "list", component: ListScreen)
                            Route(path: "detail/:id", component: Detail)
                        }
                    }
                }
            }
        }) {
            Outlet {}
            AndroidPredictiveBack {}
            SwipeBack {}
        }
    }
}

// ---------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------

#[component]
fn home() -> Element {
    let nav = use_navigator();
    render! {
        View(style: screen_style(0x101018)) {
            Text(value: "Home", style: title_style())
            Text(value: "Tab 0 · its own stack", style: subtitle_style())
            View(
                style: button_style(),
                on_tap: move |_| {
                    let _ = nav.navigate("/detail/1");
                },
            ) {
                Text(value: "Open Detail 1", style: button_label_style())
            }
        }
    }
}

#[component]
fn list_screen() -> Element {
    let nav = use_navigator();
    render! {
        View(style: screen_style(0x0E1414)) {
            Text(value: "List", style: title_style())
            Text(value: "Tab 1 · its own stack", style: subtitle_style())
            View(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| {
                        let _ = nav.navigate("/detail/42");
                    }
                },
            ) {
                Text(value: "Open Detail 42", style: button_label_style())
            }
            View(
                style: button_style(),
                on_tap: move |_| {
                    let _ = nav.navigate("/detail/99");
                },
            ) {
                Text(value: "Open Detail 99", style: button_label_style())
            }
        }
    }
}

#[component]
fn detail() -> Element {
    let nav = use_navigator();
    // Read this route's `:id` param from context — the macro-free analogue
    // of `routes! { Route("detail/:id", Detail) }` + `use_param`.
    let id = use_param("id");
    let cur = id.get().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let next = cur + 1;
    render! {
        View(style: screen_style(0x1A1422)) {
            Text(value: format!("Detail #{cur}"), style: title_style())
            Text(
                value: "Try the stack ops below. Push/Back are the baseline; \
                        Replace and Reset are what #265 / #264 fixed.",
                style: subtitle_style(),
            )

            // Push (baseline): grows the stack — slides in.
            View(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| { let _ = nav.navigate(&format!("/detail/{next}")); }
                },
            ) {
                Text(value: format!("Push → Detail #{next}"), style: button_label_style())
            }

            // Replace (#265): swaps the top in place. Must SLIDE the new
            // screen in like a push, not snap instantly.
            View(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| { let _ = nav.replace(&format!("/detail/{next}")); }
                },
            ) {
                Text(value: format!("Replace → Detail #{next}"), style: button_label_style())
            }

            // Reset to a NEW route (not in the back stack): global reset
            // collapses every stack to `[detail/5]`. Direction would be a
            // push once reset animates.
            View(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| { let _ = nav.reset("/detail/5"); }
                },
            ) {
                Text(value: "Reset → Detail #5", style: button_label_style())
            }

            // Reset to the Home tab. "/" resolves to the home index screen
            // (the "" route inside the "(home)" group), even from another tab.
            View(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| { let _ = nav.reset("/"); }
                },
            ) {
                Text(value: "Reset → Home", style: button_label_style())
            }

            View(
                style: button_style(),
                on_tap: move |_| {
                    let _ = nav.back();
                },
            ) {
                Text(value: "Back (or swipe from the left edge)", style: button_label_style())
            }
        }
    }
}

// ---------------------------------------------------------------------
// Tiny shared styles
// ---------------------------------------------------------------------

fn button_style() -> Css {
    css!(
        padding: (px(12), px(24)),
        margin_top: px(16),
        border_radius: px(12),
        background_color: Color::hex(0x7C5CFF),
    )
}

fn button_label_style() -> Css {
    css!(color: Color::hex(0xFFFFFF), font_size: px(16))
}

fn screen_style(bg: u32) -> Css {
    css!(
        flex_grow: 1.0,
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        background_color: Color::hex(bg),
    )
}

fn title_style() -> Css {
    css!(color: Color::hex(0xFFFFFF), font_size: px(28))
}

fn subtitle_style() -> Css {
    css!(color: Color::hex(0x9A9AB0), font_size: px(14), margin_top: px(8))
}
