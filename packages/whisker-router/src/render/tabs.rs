//! [`Tabs`] — the standard bottom-nav chrome for a
//! [`Switch`](crate::core::RouteTree::Switch).
//!
//! Per the design doc, the **tab bar is a `Layout`, not the `Switch`**:
//! the `Switch` is pure selection logic and draws nothing; the chrome is
//! a separate layout that renders an [`Outlet`](crate::render::Outlet)
//! for the selected branch plus a bottom bar. [`Tabs`] ships a basic
//! default bar.
//!
//! The bar **highlights itself by matching the current location** against
//! each tab's [`Target`] (the Expo-Router model — a tab is "active" when the
//! current route lives in its branch), so it needs no active-index prop or
//! hook. For fully custom chrome, read [`use_pathname`](crate::render::use_pathname)
//! and call `navigator.select(..)` yourself.

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

use crate::core::{CompiledTree, NodePath, Target};
use crate::render::components::{Layout, Outlet};
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
    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
        )) {
            // Content area: the selected branch renders here. `Layout`
            // sets the OutletAnchor to this Switch's path so the inner
            // `Outlet` draws THIS container (not whatever the ambient
            // anchor was) — the single draw path for the switch.
            view(style: css!(
                flex_grow: 1.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                Layout(path: path.clone()) {
                    Outlet {}
                }
            }
            // Bottom navigation bar — highlights the active tab itself.
            TabBar(items: items.clone())
        }
    }
}

/// The default bottom bar — a row of tappable labels, the active one
/// highlighted. Split out from [`Tabs`] so a custom layout can reuse it.
///
/// The active tab is **derived from the current location** (the item whose
/// [`Target`] lives in the active branch), not passed in — drop it into any
/// layout and it reflects navigation automatically.
#[component]
pub fn tab_bar(items: Vec<TabItem>) -> Element {
    let nav = use_navigator();

    // expo-router style: highlight the tab whose target is in the active
    // branch by matching each target against the current leaf's path.
    let active = {
        let nav = nav.clone();
        let items = items.clone();
        let current = nav.current();
        computed(move || {
            let cur = current.get().path;
            active_index(nav.tree(), &items, &cur)
        })
    };

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

/// The index of the tab whose target sits in the currently-active branch:
/// the item whose resolved path shares the **longest common prefix** with
/// the current leaf's path. All tab targets are screens in sibling branches
/// of one `Switch`, so the active branch's item shares the `Switch` *and* its
/// selected branch index with the current path — a strictly longer prefix
/// than any sibling tab (which diverges at the `Switch`). Falls back to `0`.
fn active_index(tree: &CompiledTree, items: &[TabItem], current: &NodePath) -> usize {
    items
        .iter()
        .enumerate()
        .max_by_key(|(_, item)| {
            target_path(tree, &item.target)
                .map(|p| common_prefix_len(&p, current))
                .unwrap_or(0)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// The first declaration-order path a tab `target` resolves to — its own
/// branch, independent of the current position (we want where the tab *is*,
/// not a relative pick).
fn target_path(tree: &CompiledTree, target: &Target) -> Option<NodePath> {
    match target {
        Target::Id(id) => tree.paths_with_route_id(id).into_iter().next(),
        Target::Url(url) => tree.paths_with_url(url).into_iter().next(),
    }
}

/// Length of the shared leading run of two paths' indices.
fn common_prefix_len(a: &NodePath, b: &NodePath) -> usize {
    a.0.iter()
        .zip(b.0.iter())
        .take_while(|(x, y)| x == y)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{RouteTree, SwitchDef};

    /// Switch { Stack{ "" , detail }  Stack{ "list", detail } } — two tabs,
    /// each with its own stack sharing a `detail` route.
    fn tabbed_tree() -> CompiledTree {
        CompiledTree::new(RouteTree::switch(
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
        ))
    }

    #[test]
    fn active_index_follows_the_current_branch() {
        let tree = tabbed_tree();
        let items = vec![
            TabItem::new("Home", Target::id("home")),
            TabItem::new("List", Target::id("list")),
        ];
        // Current leaf in branch 0 (home) → Home tab.
        assert_eq!(active_index(&tree, &items, &NodePath(vec![0, 0])), 0);
        // Current leaf in branch 1 (list) → List tab.
        assert_eq!(active_index(&tree, &items, &NodePath(vec![1, 0])), 1);
        // Deep in branch 1 (its detail) → still List tab (shares the branch).
        assert_eq!(active_index(&tree, &items, &NodePath(vec![1, 1])), 1);
    }
}
