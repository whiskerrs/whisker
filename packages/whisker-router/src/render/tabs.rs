//! [`TabBar`] — the default bottom navigation bar.
//!
//! There is **no all-in-one `Tabs` component**: a tab layout is just a
//! [`Layout`](crate::render::Layout) component you write (an
//! [`Outlet`](crate::render::Outlet) for the active branch above a bar), so
//! the chrome stays fully in your hands. [`TabBar`] is the ready-made bar to
//! drop into that layout — it **highlights itself by matching the current
//! location** against each tab's [`Target`] (the Expo-Router model — a tab is
//! "active" when the current route lives in its branch), so it needs no
//! active-index prop or hook. Tapping a tab calls `navigator.select(..)`.
//!
//! For a fully custom bar, read [`use_pathname`](crate::render::use_pathname)
//! and call `navigator.select(..)` yourself.

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

use crate::core::{CompiledTree, NodePath, Target};
use crate::render::handle::use_navigator;

/// One entry in the [`TabBar`]: a label + the [`Target`] selecting its branch.
///
/// `#[non_exhaustive]` so future fields (an icon, a badge) stay additive —
/// construct it with [`TabItem::new`].
#[derive(Clone)]
#[non_exhaustive]
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

/// Visual style for the built-in [`TabBar`]. Defaults to a dark
/// theme; pass a custom one to retheme the default bar without reimplementing
/// it. (For a fundamentally different bar, build your own chrome via
/// [`Layout`](crate::render::Layout) — see the module docs.)
/// Built as a **builder** off [`TabBarStyle::default`] so future style options
/// stay additive (a new field is a new setter, never a broken struct literal):
///
/// ```ignore
/// TabBar(items: items, style: TabBarStyle::default()
///     .background(Color::hex(0xFFFFFF))
///     .label_color(Color::hex(0x111111)))
/// ```
#[derive(Clone)]
pub struct TabBarStyle {
    pub(crate) background: Color,
    pub(crate) height: f32,
    pub(crate) label_color: Color,
    pub(crate) font_size: f32,
    pub(crate) active_opacity: f32,
    pub(crate) inactive_opacity: f32,
}

impl Default for TabBarStyle {
    fn default() -> Self {
        TabBarStyle {
            background: Color::hex(0x16161D),
            height: 56.0,
            label_color: Color::hex(0xFFFFFF),
            font_size: 13.0,
            active_opacity: 1.0,
            inactive_opacity: 0.5,
        }
    }
}

impl TabBarStyle {
    /// Bar background color.
    pub fn background(mut self, color: Color) -> Self {
        self.background = color;
        self
    }
    /// Bar / tab height (px).
    pub fn height(mut self, px: f32) -> Self {
        self.height = px;
        self
    }
    /// Tab label color.
    pub fn label_color(mut self, color: Color) -> Self {
        self.label_color = color;
        self
    }
    /// Tab label font size (px).
    pub fn font_size(mut self, px: f32) -> Self {
        self.font_size = px;
        self
    }
    /// Opacity of the active tab (0..1).
    pub fn active_opacity(mut self, o: f32) -> Self {
        self.active_opacity = o;
        self
    }
    /// Opacity of inactive tabs (0..1).
    pub fn inactive_opacity(mut self, o: f32) -> Self {
        self.inactive_opacity = o;
        self
    }
}

/// The default bottom bar — a row of tappable labels, the active one
/// highlighted. Drop it into your own tab [`Layout`](crate::render::Layout):
///
/// ```ignore
/// #[component]
/// fn tabs_layout() -> Element {
///     render! {
///         view(style: css!(flex_grow: 1.0, display: Display::Flex,
///                          flex_direction: FlexDirection::Column)) {
///             view(style: css!(flex_grow: 1.0)) { Outlet {} }
///             TabBar(items: vec![
///                 TabItem::new("Home",   Target::id("home")),
///                 TabItem::new("Search", Target::id("search")),
///             ])
///         }
///     }
/// }
/// // routes!: Layout(TabsLayout) { Switch { … } }
/// ```
///
/// The active tab is **derived from the current location** (the item whose
/// [`Target`] lives in the active branch), not passed in — drop it into any
/// layout and it reflects navigation automatically.
#[component]
pub fn tab_bar(
    items: Vec<TabItem>,
    #[prop(default = TabBarStyle::default())] style: TabBarStyle,
) -> Element {
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

    // Theme values (all `Copy`) lifted out so the per-item closures capture
    // them without cloning the whole style.
    let bar_bg = style.background;
    let item_h = style.height;
    let label_color = style.label_color;
    let font_size = style.font_size;
    let active_op = style.active_opacity;
    let inactive_op = style.inactive_opacity;

    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceAround,
            align_items: AlignItems::Center,
            height: px(item_h),
            background_color: bar_bg,
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
                                let opacity = if active.get() == i {
                                    active_op
                                } else {
                                    inactive_op
                                };
                                css!(
                                    flex_grow: 1.0,
                                    display: Display::Flex,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    height: px(item_h),
                                )
                                .raw("opacity", opacity.to_string())
                            }),
                            on_tap: move |_| {
                                let _ = nav.select(&target);
                            },
                        ) {
                            text(
                                value: item.label.clone(),
                                style: css!(color: label_color, font_size: px(font_size)),
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
