//! Built-in authoring tag identifiers used by the `render!` macro.

/// Legacy authoring tag IDs retained for source compatibility.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementTag {
    /// Legacy shell root. Not a public Whisker element registration.
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
