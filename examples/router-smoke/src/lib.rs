//! router-smoke — on-device check of the new whisker-router rendering
//! layer (phase 2).
//!
//! A tabbed app, hand-wired (no `routes!` macro yet). The **Switch is the
//! root** so the whole tree is drawn on a single path by one `Tabs`:
//!
//! ```text
//! Switch (root, drawn by Tabs)
//!   ├ branch 0 [0]  Stack { Route("", home)       Route("detail/:id", detail) }
//!   └ branch 1 [1]  Stack { Route("list", list)   Route("detail/:id", detail) }
//! ```
//!
//! - **Home tab**: a button → `navigate(detail)` pushes a Detail onto the
//!   Home stack (Slide transition). Detail has a Back button → `back()`.
//! - **List tab**: rows → `navigate(detail)` pushes Detail onto the List
//!   stack. Each tab keeps its **own** history (switching tabs preserves
//!   where you were), and the shared `detail` route dedupes to whichever
//!   tab you are in (relative resolution).
//! - **Tabs bar**: the persistent bottom chrome (a `Layout`, not the
//!   `Switch`) — tapping a tab calls `navigator.select(..)`.
//! - **Swipe-back**: an iOS edge swipe pops the active stack with a
//!   velocity hand-off.
//!
//! `Router` only publishes context + renders its children; the tree is
//! drawn **once** by the `Tabs` child (an outside route stacked above the
//! tabs would need a wrapping root `Stack` + a Layout node, which the
//! `routes!` macro will generate in phase 3).

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_router::core::{CompiledTree, NodePath, RouteInstance, RouteTree, SwitchDef, Target};
use whisker_router::render::{
    AndroidPredictiveBack, RouteRegistry, Router, RouterHandle, SwipeBack, TabItem, Tabs,
    use_navigator, use_param,
};

/// The Switch (tabs) is the tree root, so its path is the root path.
fn tabs_switch_path() -> NodePath {
    NodePath::root()
}

fn build_handle() -> RouterHandle {
    // The Switch is the root; each branch is its own Stack. Drawing the
    // root (via the single `Tabs` child) draws the whole tree once.
    let tree = CompiledTree::new(RouteTree::switch(
        SwitchDef::new("tabs", 0),
        vec![
            RouteTree::stack(vec![
                RouteTree::route("", "home"),
                RouteTree::route("detail/:id", "detail"),
            ]),
            RouteTree::stack(vec![
                RouteTree::route("list", "list"),
                RouteTree::route("detail/:id", "detail"),
            ]),
        ],
    ));

    let registry = RouteRegistry::new()
        .route("home", |_: &RouteInstance| render! { Home {} })
        .route("list", |_: &RouteInstance| render! { ListScreen {} })
        // Default transition: the platform default — full iOS slide on
        // iOS, the subtler small-slide + fade on Android. The closure no
        // longer extracts `:id`; `Detail` reads it itself via `use_param`.
        .route("detail", |_: &RouteInstance| render! { Detail {} });

    RouterHandle::new(tree, registry)
}

#[whisker::main]
fn app() -> Element {
    let handle = build_handle();
    render! {
        Router(handle: handle) {
            // Tab chrome is a Layout wrapping the Switch's Outlet.
            Tabs(
                path: tabs_switch_path(),
                items: vec![
                    TabItem::new("Home", Target::id("home")),
                    TabItem::new("List", Target::id("list")),
                ],
            )
            // Interactive back gestures — both mounted; each waits on its
            // own platform input. iOS = leading-edge swipe; Android =
            // system predictive back (13+ shows the Material live preview:
            // the current screen shrinks/rounds and slides, the previous
            // screen peeks beside it, the backdrop dims).
            AndroidPredictiveBack {}
            SwipeBack {}
        }
    }
}

// ---------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------

fn detail_target(id: &str) -> (Target, RouteInstance) {
    (
        Target::id("detail"),
        RouteInstance::with_param(NodePath::root(), "id", id),
    )
}

#[component]
fn home() -> Element {
    let nav = use_navigator();
    render! {
        view(style: screen_style(0x101018)) {
            text(value: "Home", style: title_style())
            text(value: "Tab 0 · its own stack", style: subtitle_style())
            view(
                style: button_style(),
                on_tap: move |_| {
                    let (t, inst) = detail_target("1");
                    let _ = nav.navigate_with(&t, inst);
                },
            ) {
                text(value: "Open Detail 1", style: button_label_style())
            }
        }
    }
}

#[component]
fn list_screen() -> Element {
    let nav = use_navigator();
    render! {
        view(style: screen_style(0x0E1414)) {
            text(value: "List", style: title_style())
            text(value: "Tab 1 · its own stack", style: subtitle_style())
            view(
                style: button_style(),
                on_tap: {
                    let nav = nav.clone();
                    move |_| {
                        let (t, inst) = detail_target("42");
                        let _ = nav.navigate_with(&t, inst);
                    }
                },
            ) {
                text(value: "Open Detail 42", style: button_label_style())
            }
            view(
                style: button_style(),
                on_tap: move |_| {
                    let (t, inst) = detail_target("99");
                    let _ = nav.navigate_with(&t, inst);
                },
            ) {
                text(value: "Open Detail 99", style: button_label_style())
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
    let label = format!("Detail #{}", id.get().unwrap_or_default());
    render! {
        view(style: screen_style(0x1A1422)) {
            text(value: label, style: title_style())
            text(value: "Swipe from the left edge, or tap Back.", style: subtitle_style())
            view(
                style: button_style(),
                on_tap: move |_| {
                    nav.back();
                },
            ) {
                text(value: "Back", style: button_label_style())
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
