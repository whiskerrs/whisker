//! [`Pane`] — display-toggleable container that keeps its children
//! mounted while hidden.
//!
//! The building block behind [`TabsLayout`](crate::TabsLayout). Use
//! directly when you want keep-alive semantics without `TabsLayout`'s
//! `(matches, content)` shape — e.g. a master-detail surface where
//! one of two panes is shown at a time without unmounting the other.
//!
//! Inactive panes' state (scroll position, form input, in-flight
//! effects) survives the toggle because their elements are never
//! unmounted — only their `display` flips between `flex` and `none`.
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_router::Pane;
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
/// `visible` is a `Fn() -> bool` — same shape as [`whisker::Show`]'s
/// `when`, so the call site writes a closure over its reactive
/// dependencies.
///
/// See the [module docs](self) for a tabbed-pane example.
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
