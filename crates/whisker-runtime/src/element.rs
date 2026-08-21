//! Element tag enum, shared between the `render!` macro emit and
//! the bridge tag-mapping table.

/// Element tag. Numeric repr stays in sync with `WhiskerElementTag`
/// in `crates/whisker-driver-sys/bridge/include/whisker_bridge.h`.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementTag {
    /// Legacy Lynx shell root. Not a public Whisker element registration.
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
