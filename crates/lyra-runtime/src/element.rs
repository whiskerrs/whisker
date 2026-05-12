//! The `Element` data structure that user code (or the `rsx!` macro)
//! produces. Elements are pure data: they own their attributes, styles,
//! event handlers, and children, and have no live connection to a renderer
//! until [`crate::render::render`] (or [`crate::diff::diff`]) walks them.

use std::sync::Arc;

/// Maps to the integer tag the bridge accepts. Keep `#[repr(u32)]` in sync
/// with `LyraElementTag` in `native/bridge/include/lyra_bridge.h`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    Image = 5,
}

impl ElementTag {
    pub fn name(self) -> &'static str {
        match self {
            ElementTag::Page => "page",
            ElementTag::View => "view",
            ElementTag::Text => "text",
            ElementTag::RawText => "raw-text",
            ElementTag::Image => "image",
        }
    }
}

/// An attribute name/value pair. Sorted by `name` to make diffing
/// deterministic without hashing per-pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
}

/// Event handler payload. Boxed `Fn` so closures of any captured-state
/// shape fit in the same slot. `Arc` so cloning an [`Element`] doesn't
/// duplicate the closure.
#[derive(Clone)]
pub struct EventHandler {
    pub name: String,
    pub callback: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for EventHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventHandler")
            .field("name", &self.name)
            .field("callback", &"<fn>")
            .finish()
    }
}

// EventHandlers compare equal iff the name matches; closure pointers are
// not stable enough to compare meaningfully. Diff treats event-handler
// changes as "always replace the binding", which is correct.
impl PartialEq for EventHandler {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for EventHandler {}

/// One node in a Lyra element tree. Trees are entirely owned (no
/// reference-counting between nodes); cloning an [`Element`] deep-clones
/// everything except event-handler closures (which are `Arc`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: ElementTag,
    /// Optional key for diff stability when sibling order may change.
    pub key: Option<String>,
    /// Sorted by `Attribute::name`.
    pub attrs: Vec<Attribute>,
    /// Raw inline-style CSS string. Empty if no styles were set.
    pub styles: String,
    pub events: Vec<EventHandler>,
    pub children: Vec<Element>,
}

impl Element {
    pub fn new(tag: ElementTag) -> Self {
        Self {
            tag,
            key: None,
            attrs: Vec::new(),
            styles: String::new(),
            events: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Lookup an attribute by name. O(log n) thanks to the sorted invariant.
    pub fn get_attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .binary_search_by(|a| a.name.as_str().cmp(name))
            .ok()
            .map(|i| self.attrs[i].value.as_str())
    }

    /// Insert (or replace) an attribute, preserving the sorted order.
    pub fn insert_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self.attrs.binary_search_by(|a| a.name.cmp(&name)) {
            Ok(i) => self.attrs[i].value = value,
            Err(i) => self.attrs.insert(i, Attribute { name, value }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_name_is_stable() {
        assert_eq!(ElementTag::Page.name(), "page");
        assert_eq!(ElementTag::View.name(), "view");
        assert_eq!(ElementTag::Text.name(), "text");
        assert_eq!(ElementTag::RawText.name(), "raw-text");
        assert_eq!(ElementTag::Image.name(), "image");
    }

    #[test]
    fn tag_repr_matches_bridge_constants() {
        // These integer values are part of the C ABI contract — see
        // native/bridge/include/lyra_bridge.h.
        assert_eq!(ElementTag::Page as u32, 1);
        assert_eq!(ElementTag::View as u32, 2);
        assert_eq!(ElementTag::Text as u32, 3);
        assert_eq!(ElementTag::RawText as u32, 4);
        assert_eq!(ElementTag::Image as u32, 5);
    }

    #[test]
    fn insert_attr_keeps_sorted_invariant() {
        let mut e = Element::new(ElementTag::View);
        e.insert_attr("zindex", "10");
        e.insert_attr("aria-label", "hi");
        e.insert_attr("class", "btn");

        let names: Vec<_> = e.attrs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["aria-label", "class", "zindex"]);
    }

    #[test]
    fn insert_attr_replaces_existing() {
        let mut e = Element::new(ElementTag::Text);
        e.insert_attr("x", "1");
        e.insert_attr("x", "2");
        e.insert_attr("x", "3");
        assert_eq!(e.attrs.len(), 1);
        assert_eq!(e.get_attr("x"), Some("3"));
    }

    #[test]
    fn attr_lookup() {
        let mut e = Element::new(ElementTag::View);
        e.insert_attr("a", "1");
        e.insert_attr("b", "2");
        assert_eq!(e.get_attr("a"), Some("1"));
        assert_eq!(e.get_attr("b"), Some("2"));
        assert_eq!(e.get_attr("c"), None);
    }

    #[test]
    fn elements_are_value_comparable() {
        let mut a = Element::new(ElementTag::View);
        a.insert_attr("class", "x");
        let mut b = Element::new(ElementTag::View);
        b.insert_attr("class", "x");
        assert_eq!(a, b);
    }

    #[test]
    fn elements_with_different_styles_differ() {
        let mut a = Element::new(ElementTag::View);
        a.styles = "color: red".into();
        let mut b = Element::new(ElementTag::View);
        b.styles = "color: blue".into();
        assert_ne!(a, b);
    }

    #[test]
    fn event_handlers_compare_by_name_only() {
        let h1 = EventHandler {
            name: "click".into(),
            callback: Arc::new(|| {}),
        };
        let h2 = EventHandler {
            name: "click".into(),
            callback: Arc::new(|| {}),
        };
        assert_eq!(h1, h2);

        let h3 = EventHandler {
            name: "tap".into(),
            callback: Arc::clone(&h1.callback),
        };
        assert_ne!(h1, h3);
    }
}
