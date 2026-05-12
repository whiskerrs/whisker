//! Backend abstraction.
//!
//! Anything that can construct a Lyra element tree implements [`Renderer`].
//! Production builds use a `BridgeRenderer` (defined in `lyra-mobile`) that
//! talks to the C++ bridge via FFI; tests use [`MockRenderer`] which records
//! ops in a `Vec` so cargo tests don't need iOS/Android infrastructure.

use crate::element::ElementTag;

/// Backend that materializes an element tree.
///
/// All methods take `&mut self` — implementors are not expected to be
/// thread-safe; calling code is responsible for sequencing.
pub trait Renderer {
    /// Opaque per-element handle. `Copy`/`Eq` to make tests trivial and
    /// because the production handle is a pointer.
    type ElementHandle: Copy + Eq + std::fmt::Debug;

    fn create_element(&mut self, tag: ElementTag) -> Self::ElementHandle;
    fn release_element(&mut self, handle: Self::ElementHandle);

    fn set_attribute(&mut self, handle: Self::ElementHandle, key: &str, value: &str);
    fn set_inline_styles(&mut self, handle: Self::ElementHandle, css: &str);

    fn append_child(
        &mut self,
        parent: Self::ElementHandle,
        child: Self::ElementHandle,
    );
    fn remove_child(
        &mut self,
        parent: Self::ElementHandle,
        child: Self::ElementHandle,
    );

    /// Make this element the engine's root (must be a Page).
    fn set_root(&mut self, page: Self::ElementHandle);

    /// Run a frame: resolve / layout / paint.
    fn flush(&mut self);
}

// ----------------------------------------------------------------------------
// Mock renderer for tests.
// ----------------------------------------------------------------------------

/// Operation recorded by [`MockRenderer`]. Tests assert on a `Vec<MockOp>`
/// to verify that builder/diff code emits the expected sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockOp {
    Create { handle: u32, tag: ElementTag },
    Release { handle: u32 },
    SetAttribute { handle: u32, key: String, value: String },
    SetInlineStyles { handle: u32, css: String },
    AppendChild { parent: u32, child: u32 },
    RemoveChild { parent: u32, child: u32 },
    SetRoot { page: u32 },
    Flush,
}

/// In-memory recording renderer used by tests.
///
/// Hands out monotonically increasing `u32` handles starting at 1 (so 0 is
/// always invalid and easy to spot in test failures).
#[derive(Debug, Default)]
pub struct MockRenderer {
    next_handle: u32,
    ops: Vec<MockOp>,
}

impl MockRenderer {
    pub fn new() -> Self {
        Self {
            next_handle: 0,
            ops: Vec::new(),
        }
    }

    pub fn ops(&self) -> &[MockOp] {
        &self.ops
    }

    pub fn into_ops(self) -> Vec<MockOp> {
        self.ops
    }
}

impl Renderer for MockRenderer {
    type ElementHandle = u32;

    fn create_element(&mut self, tag: ElementTag) -> Self::ElementHandle {
        self.next_handle += 1;
        let handle = self.next_handle;
        self.ops.push(MockOp::Create { handle, tag });
        handle
    }

    fn release_element(&mut self, handle: Self::ElementHandle) {
        self.ops.push(MockOp::Release { handle });
    }

    fn set_attribute(&mut self, handle: Self::ElementHandle, key: &str, value: &str) {
        self.ops.push(MockOp::SetAttribute {
            handle,
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }

    fn set_inline_styles(&mut self, handle: Self::ElementHandle, css: &str) {
        self.ops.push(MockOp::SetInlineStyles {
            handle,
            css: css.to_owned(),
        });
    }

    fn append_child(
        &mut self,
        parent: Self::ElementHandle,
        child: Self::ElementHandle,
    ) {
        self.ops.push(MockOp::AppendChild { parent, child });
    }

    fn remove_child(
        &mut self,
        parent: Self::ElementHandle,
        child: Self::ElementHandle,
    ) {
        self.ops.push(MockOp::RemoveChild { parent, child });
    }

    fn set_root(&mut self, page: Self::ElementHandle) {
        self.ops.push(MockOp::SetRoot { page });
    }

    fn flush(&mut self) {
        self.ops.push(MockOp::Flush);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_monotonic_and_nonzero() {
        let mut r = MockRenderer::new();
        let a = r.create_element(ElementTag::View);
        let b = r.create_element(ElementTag::Text);
        let c = r.create_element(ElementTag::RawText);

        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
        assert_ne!(a, 0, "0 should be reserved as 'invalid'");
    }

    #[test]
    fn ops_record_in_call_order() {
        let mut r = MockRenderer::new();
        let p = r.create_element(ElementTag::Page);
        let v = r.create_element(ElementTag::View);
        r.set_inline_styles(p, "background: white");
        r.append_child(p, v);
        r.set_root(p);
        r.flush();

        assert_eq!(
            r.ops(),
            &[
                MockOp::Create { handle: 1, tag: ElementTag::Page },
                MockOp::Create { handle: 2, tag: ElementTag::View },
                MockOp::SetInlineStyles {
                    handle: 1,
                    css: "background: white".into(),
                },
                MockOp::AppendChild { parent: 1, child: 2 },
                MockOp::SetRoot { page: 1 },
                MockOp::Flush,
            ]
        );
    }

    #[test]
    fn release_and_remove_record_correctly() {
        let mut r = MockRenderer::new();
        let p = r.create_element(ElementTag::Page);
        let v = r.create_element(ElementTag::View);
        r.append_child(p, v);
        r.remove_child(p, v);
        r.release_element(v);

        let ops = r.into_ops();
        assert!(matches!(ops[3], MockOp::RemoveChild { parent: 1, child: 2 }));
        assert!(matches!(ops[4], MockOp::Release { handle: 2 }));
    }
}
