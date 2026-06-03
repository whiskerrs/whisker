//! Element tag enum, shared between the `render!` macro emit and
//! the bridge tag-mapping table.
//!
//! Pre-Phase-6.5a this module also held the `Element` value-tree
//! struct + attribute / event / child data and the diff/patch flow
//! consumed by `render.rs`. That entire pipeline retired with A3 —
//! the fine-grained reactive runtime mutates the Lynx element tree
//! directly through `view::*` rather than building an intermediate
//! Rust-side data structure. Only the tag enum survives because the
//! C bridge still keys element creation off it.

/// Element tag. Numeric repr stays in sync with `WhiskerElementTag`
/// in `crates/whisker-driver-sys/bridge/include/whisker_bridge.h`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementTag {
    Page = 1,
    View = 2,
    Text = 3,
    RawText = 4,
    ScrollView = 5,
}

impl ElementTag {
    pub fn name(self) -> &'static str {
        match self {
            ElementTag::Page => "page",
            ElementTag::View => "view",
            ElementTag::Text => "text",
            ElementTag::RawText => "raw-text",
            ElementTag::ScrollView => "scroll-view",
        }
    }
}
