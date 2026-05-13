//! [`Patch`] — the diff engine's output. Each variant maps directly onto a
//! [`crate::renderer::Renderer`] call (or a small group of them).
//!
//! Patches are tagged with a node "path" — a sequence of child indices
//! starting at the root — so the apply step can look up the right
//! renderer handle without rewalking the new tree.

use crate::element::{Element, EventHandler};

/// Path from root to a node, expressed as child indices. `[]` is the root,
/// `[0]` is the root's first child, `[0, 2]` is the third grandchild via
/// the first child, and so on.
pub type Path = Vec<usize>;

#[derive(Debug, Clone, PartialEq)]
pub enum Patch {
    /// Replace the subtree at `path` with a fresh subtree built from the
    /// new node. The renderer creates/releases handles as needed.
    Replace { path: Path, new: Element },

    /// Append `node` as a new child at `path` (parent path).
    AppendChild { parent: Path, node: Element },

    /// Remove the child at index `child_index` of the node at `parent`.
    RemoveChild { parent: Path, child_index: usize },

    /// Insert `node` before the child at `child_index` of the node at
    /// `parent`. (Used for keyed reorders / mid-list inserts.)
    InsertChildBefore {
        parent: Path,
        child_index: usize,
        node: Element,
    },

    /// Set or replace an attribute on the node at `path`.
    SetAttribute {
        path: Path,
        name: String,
        value: String,
    },

    /// Remove an attribute from the node at `path`.
    RemoveAttribute { path: Path, name: String },

    /// Replace the inline-style string on the node at `path`.
    SetInlineStyles { path: Path, css: String },

    /// Replace the event-handler list on the node at `path`. Diff treats
    /// any change to the event set as a wholesale replacement.
    ReplaceEvents { path: Path, events: Vec<EventHandler> },
}

impl Patch {
    /// Convenience for tests: returns a stable identifier for the patch
    /// variant, useful when asserting patch counts/types without caring
    /// about every payload byte.
    pub fn kind(&self) -> &'static str {
        match self {
            Patch::Replace { .. } => "Replace",
            Patch::AppendChild { .. } => "AppendChild",
            Patch::RemoveChild { .. } => "RemoveChild",
            Patch::InsertChildBefore { .. } => "InsertChildBefore",
            Patch::SetAttribute { .. } => "SetAttribute",
            Patch::RemoveAttribute { .. } => "RemoveAttribute",
            Patch::SetInlineStyles { .. } => "SetInlineStyles",
            Patch::ReplaceEvents { .. } => "ReplaceEvents",
        }
    }
}

/// Reason a node had to be replaced rather than patched in place. Used
/// internally; kept public so tests / callers can introspect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceReason {
    TagChanged,
    KeyChanged,
}

/// Helper for the diff engine: returns true if the two nodes can be
/// reconciled in place (same tag and matching key).
pub fn can_reconcile(a: &Element, b: &Element) -> Option<ReplaceReason> {
    if a.tag != b.tag {
        Some(ReplaceReason::TagChanged)
    } else if a.key != b.key {
        Some(ReplaceReason::KeyChanged)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::*;

    #[test]
    fn can_reconcile_same_tag_and_key() {
        let a = view().key("k");
        let b = view().key("k");
        assert_eq!(can_reconcile(&a, &b), None);
    }

    #[test]
    fn can_reconcile_no_keys() {
        let a = view();
        let b = view();
        assert_eq!(can_reconcile(&a, &b), None);
    }

    #[test]
    fn cannot_reconcile_different_tag() {
        let a = view();
        let b = text();
        assert_eq!(can_reconcile(&a, &b), Some(ReplaceReason::TagChanged));
    }

    #[test]
    fn cannot_reconcile_different_keys() {
        let a = view().key("a");
        let b = view().key("b");
        assert_eq!(can_reconcile(&a, &b), Some(ReplaceReason::KeyChanged));
    }

    #[test]
    fn key_added_or_removed_blocks_reconciliation() {
        let a = view();
        let b = view().key("x");
        assert_eq!(can_reconcile(&a, &b), Some(ReplaceReason::KeyChanged));
        assert_eq!(can_reconcile(&b, &a), Some(ReplaceReason::KeyChanged));
    }

    #[test]
    fn patch_kind_strings_are_stable() {
        let p = Patch::AppendChild { parent: vec![], node: view() };
        assert_eq!(p.kind(), "AppendChild");
    }
}
