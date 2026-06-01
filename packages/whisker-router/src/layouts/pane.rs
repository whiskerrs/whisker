//! `Pane` — display-toggleable container that keeps its children
//! mounted while hidden.
//!
//! Tabs are the canonical caller: each tab's content lives inside
//! a `Pane`, only one is visible at a time, and the inactive
//! panes' state (scroll position, form inputs, in-flight effects)
//! survives the switch because their elements are never unmounted.
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_router::layouts::Pane;
//!
//! let tab = RwSignal::new(Tab::Home);
//!
//! render! {
//!     view(style: css!(flex_grow: 1)) {
//!         Pane(visible: move || tab.get() == Tab::Home)    { Home() }
//!         Pane(visible: move || tab.get() == Tab::Search)  { Search() }
//!         Pane(visible: move || tab.get() == Tab::Profile) { Profile() }
//!     }
//! }
//! ```

use whisker::css::ext::*;
use whisker::css::{Css, Display, FlexDirection, ToCss};
use whisker::runtime::view::Element;
use whisker::{component, computed, Children, WhenFn};

/// Container that toggles between `display: flex` (children visible)
/// and `display: none` (children hidden but still mounted).
///
/// `visible` is a `Fn() -> bool` — the call site writes a closure
/// over its reactive deps, same shape as `Show`'s `when`.
#[component]
pub fn pane(visible: WhenFn, children: Children) -> Element {
    let visible = visible.clone();
    let style = computed(move || {
        let on = visible.call();
        let mut css = Css::new()
            .flex_direction(FlexDirection::Column)
            .width(100.percent())
            .height(100.percent());
        css = if on {
            css.display(Display::Flex)
        } else {
            css.display(Display::None)
        };
        css.to_css_string()
    });
    whisker::render! {
        view(style: style) {
            children()
        }
    }
}
