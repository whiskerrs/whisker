//! [`Tabs`] — the standard bottom-nav chrome for a
//! [`Switch`](crate::core::RouteTree::Switch).
//!
//! Per the design doc, the **tab bar is a `Layout`, not the `Switch`**:
//! the `Switch` is pure selection logic and draws nothing; the chrome is
//! a separate layout that renders an [`Outlet`](crate::render::Outlet)
//! for the selected branch plus a bottom bar. [`Tabs`] ships a basic
//! default bar; for custom chrome (top tabs, a segmented control) use
//! [`Layout`](crate::render::Layout) + your own bar and call
//! [`use_active_tab`](crate::render::use_active_tab) /
//! `navigator.select(..)` yourself.

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

use crate::core::{NodePath, Target};
use crate::render::components::{Outlet, use_active_tab};
use crate::render::handle::use_navigator;

/// One entry in the [`Tabs`] bar: a label + the [`Target`] selecting its
/// branch.
#[derive(Clone)]
pub struct TabItem {
    /// The text shown in the bar.
    pub label: String,
    /// The nav target whose branch this tab selects (resolved relative
    /// to the current position by `navigator.select`).
    pub target: Target,
}

impl TabItem {
    /// A tab labelled `label` selecting `target`.
    pub fn new(label: impl Into<String>, target: Target) -> Self {
        TabItem {
            label: label.into(),
            target,
        }
    }
}

/// Standard tabs layout: the selected branch's content above a fixed
/// bottom bar.
///
/// `path` is the [`Switch`](crate::core::RouteTree::Switch)'s
/// [`NodePath`]; `items` are the bar entries in branch (declaration)
/// order. The bar reflects the active branch and calls
/// `navigator.select(target)` on tap.
///
/// ```ignore
/// render! {
///     Tabs(path: switch_path, items: vec![
///         TabItem::new("Home",   Target::id("home")),
///         TabItem::new("Search", Target::id("search")),
///     ])
/// }
/// ```
#[component]
pub fn tabs(path: NodePath, items: Vec<TabItem>) -> Element {
    let active = use_active_tab(path.clone());

    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            // Content area: the selected branch renders here.
            view(style: css!(
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                Outlet {}
            }
            // Bottom navigation bar.
            TabBar(items: items.clone(), active: active)
        }
    }
}

/// The default bottom bar — a row of tappable labels, the active one
/// highlighted. Split out from [`Tabs`] so a custom layout can reuse it.
#[component]
pub fn tab_bar(items: Vec<TabItem>, active: ReadSignal<usize>) -> Element {
    let nav = use_navigator();

    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceAround,
            align_items: AlignItems::Center,
            height: px(56),
            background_color: Color::hex(0x16161D),
        )) {
            ForEach(
                each: {
                    let items = items.clone();
                    move || items.clone().into_iter().enumerate().collect::<Vec<_>>()
                },
                key: |(i, _): &(usize, TabItem)| *i,
                children: move |(i, item): (usize, TabItem)| {
                    let nav = nav.clone();
                    let target = item.target.clone();
                    let active = active;
                    render! {
                        view(
                            style: computed(move || {
                                let on = active.get() == i;
                                css!(
                                    flex_grow: 1.0,
                                    display: Display::Flex,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    height: px(56),
                                )
                                .raw("opacity", if on { "1.0" } else { "0.5" })
                            }),
                            on_tap: move |_| {
                                let _ = nav.select(&target);
                            },
                        ) {
                            text(
                                value: item.label.clone(),
                                style: css!(color: Color::hex(0xFFFFFF), font_size: px(13)),
                            )
                        }
                    }
                },
            )
        }
    }
}
