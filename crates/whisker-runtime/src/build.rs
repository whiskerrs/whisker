//! Builder helpers — the ergonomic surface for constructing [`Element`]
//! trees by hand.
//!
//! The `rsx!` macro (Phase 7) desugars to these calls, so anything you
//! can write in `rsx!` you can also write here.
//!
//! ```
//! use whisker_runtime::prelude::*;
//!
//! let tree = page()
//!     .style("background-color: white;")
//!     .child(
//!         text()
//!             .style("font-size: 32px; color: black;")
//!             .child(raw_text("Hello"))
//!     );
//! assert_eq!(tree.tag, ElementTag::Page);
//! assert_eq!(tree.children.len(), 1);
//! ```

use crate::element::{Element, ElementTag, EventHandler};
use std::sync::Arc;

// ---- Constructors --------------------------------------------------------

pub fn page() -> Element {
    Element::new(ElementTag::Page)
}

pub fn view() -> Element {
    Element::new(ElementTag::View)
}

pub fn text() -> Element {
    Element::new(ElementTag::Text)
}

/// A `<text>` containing one `<raw-text text="...">` child. Covers the
/// 99% case for static strings; for more complex text composition build
/// the tree explicitly.
pub fn text_with(content: impl Into<String>) -> Element {
    text().child(raw_text(content))
}

pub fn raw_text(content: impl Into<String>) -> Element {
    let mut e = Element::new(ElementTag::RawText);
    e.insert_attr("text", content);
    e
}

pub fn image() -> Element {
    Element::new(ElementTag::Image)
}

pub fn scroll_view() -> Element {
    Element::new(ElementTag::ScrollView)
}

// ---- Element builder methods --------------------------------------------

impl Element {
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }

    pub fn style(mut self, css: impl Into<String>) -> Self {
        self.styles = css.into();
        self
    }

    pub fn attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert_attr(name, value);
        self
    }

    pub fn child(mut self, child: Element) -> Self {
        self.children.push(child);
        self
    }

    /// Add many children at once. Convenient with `vec![...]` or iterators.
    pub fn children(mut self, children: impl IntoIterator<Item = Element>) -> Self {
        self.children.extend(children);
        self
    }

    /// Attach an event listener. Multiple handlers with the same `name`
    /// are allowed (the diff treats them as a single "binding" replacement).
    pub fn on(
        mut self,
        name: impl Into<String>,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.events.push(EventHandler {
            name: name.into(),
            callback: Arc::new(callback),
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_produce_correct_tags() {
        assert_eq!(page().tag, ElementTag::Page);
        assert_eq!(view().tag, ElementTag::View);
        assert_eq!(text().tag, ElementTag::Text);
        assert_eq!(raw_text("").tag, ElementTag::RawText);
        assert_eq!(image().tag, ElementTag::Image);
    }

    #[test]
    fn raw_text_sets_text_attribute() {
        let r = raw_text("Hello");
        assert_eq!(r.get_attr("text"), Some("Hello"));
    }

    #[test]
    fn text_with_wraps_string_in_raw_text() {
        let t = text_with("hi");
        assert_eq!(t.tag, ElementTag::Text);
        assert_eq!(t.children.len(), 1);
        assert_eq!(t.children[0].tag, ElementTag::RawText);
        assert_eq!(t.children[0].get_attr("text"), Some("hi"));
    }

    #[test]
    fn style_replaces_previous() {
        let e = view().style("color: red").style("color: blue");
        assert_eq!(e.styles, "color: blue");
    }

    #[test]
    fn child_appends_to_children() {
        let e = view().child(text()).child(image());
        let tags: Vec<_> = e.children.iter().map(|c| c.tag).collect();
        assert_eq!(tags, [ElementTag::Text, ElementTag::Image]);
    }

    #[test]
    fn children_extends_at_once() {
        let e = view().children(vec![text(), text(), text()]);
        assert_eq!(e.children.len(), 3);
    }

    #[test]
    fn key_is_stored() {
        let e = view().key("row-7");
        assert_eq!(e.key.as_deref(), Some("row-7"));
    }

    #[test]
    fn on_records_event_handler() {
        let e = view().on("click", || {});
        assert_eq!(e.events.len(), 1);
        assert_eq!(e.events[0].name, "click");
    }

    #[test]
    fn full_tree_compiles_to_expected_structure() {
        let tree = page()
            .style("background-color: white;")
            .child(
                view()
                    .style("padding: 16px;")
                    .child(text_with("Hello Whisker")),
            );

        assert_eq!(tree.tag, ElementTag::Page);
        assert_eq!(tree.styles, "background-color: white;");
        assert_eq!(tree.children.len(), 1);

        let row = &tree.children[0];
        assert_eq!(row.tag, ElementTag::View);
        assert_eq!(row.children.len(), 1);

        let text_el = &row.children[0];
        assert_eq!(text_el.tag, ElementTag::Text);
        assert_eq!(text_el.children[0].get_attr("text"), Some("Hello Whisker"));
    }
}
