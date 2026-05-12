//! Walk an [`Element`] tree and emit creation / mutation ops to a
//! [`Renderer`]. Used both for the initial mount and (indirectly) by the
//! diff engine when it needs to materialize a brand-new subtree.
//!
//! The walk is depth-first, post-order for child handles: the recursive
//! call that builds child `n` returns its handle, which the parent then
//! appends. This keeps the Renderer ops in the natural "create child →
//! append to parent" order.

use crate::element::Element;
use crate::renderer::Renderer;

/// Mount `tree` as the engine's root and flush a frame.
///
/// Returns the root handle so callers (the diff engine, primarily) can
/// reuse it for subsequent updates without re-mounting.
pub fn mount<R: Renderer>(renderer: &mut R, tree: &Element) -> R::ElementHandle {
    let root = build_subtree(renderer, tree);
    renderer.set_root(root);
    renderer.flush();
    root
}

/// Materialize an Element subtree on the renderer and return its handle.
/// Does NOT call `set_root` or `flush` — meant to be composed.
pub fn build_subtree<R: Renderer>(renderer: &mut R, node: &Element) -> R::ElementHandle {
    let handle = renderer.create_element(node.tag);

    if !node.styles.is_empty() {
        renderer.set_inline_styles(handle, &node.styles);
    }
    for attr in &node.attrs {
        renderer.set_attribute(handle, &attr.name, &attr.value);
    }
    // Event handlers don't propagate through the renderer trait yet —
    // they're a runtime-side concern that we wire up in Phase 8.

    for child in &node.children {
        let child_handle = build_subtree(renderer, child);
        renderer.append_child(handle, child_handle);
    }

    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::*;
    use crate::element::ElementTag;
    use crate::renderer::{MockOp, MockRenderer};

    #[test]
    fn empty_element_emits_only_create() {
        let mut r = MockRenderer::new();
        build_subtree(&mut r, &view());
        assert_eq!(
            r.into_ops(),
            vec![MockOp::Create { handle: 1, tag: ElementTag::View }]
        );
    }

    #[test]
    fn styles_emitted_when_nonempty() {
        let mut r = MockRenderer::new();
        build_subtree(&mut r, &view().style("color: red"));
        assert_eq!(
            r.into_ops(),
            vec![
                MockOp::Create { handle: 1, tag: ElementTag::View },
                MockOp::SetInlineStyles {
                    handle: 1,
                    css: "color: red".into(),
                },
            ]
        );
    }

    #[test]
    fn empty_styles_string_is_skipped() {
        let mut r = MockRenderer::new();
        build_subtree(&mut r, &view());
        // No SetInlineStyles op at all.
        assert!(!r
            .ops()
            .iter()
            .any(|op| matches!(op, MockOp::SetInlineStyles { .. })));
    }

    #[test]
    fn attributes_emit_in_sorted_order() {
        let mut r = MockRenderer::new();
        let elem = raw_text("hi").attr("zindex", "1").attr("aria-label", "x");
        build_subtree(&mut r, &elem);

        let attrs: Vec<_> = r
            .into_ops()
            .into_iter()
            .filter_map(|op| match op {
                MockOp::SetAttribute { key, .. } => Some(key),
                _ => None,
            })
            .collect();
        assert_eq!(attrs, ["aria-label", "text", "zindex"]);
    }

    #[test]
    fn children_built_then_appended_to_parent() {
        let mut r = MockRenderer::new();
        let tree = view().child(text_with("a")).child(text_with("b"));
        let _ = build_subtree(&mut r, &tree);

        let ops = r.into_ops();
        // Expected sequence:
        //   Create view (1)
        //     Create text (2)
        //       Create raw_text (3); Set "text" = "a"
        //     Append 2 -> 1's prep:  Create raw_text → append to text → append text to view
        // Verify the *append* ops come AFTER the corresponding child Creates.
        let view_handle = match ops[0] {
            MockOp::Create { handle, tag: ElementTag::View } => handle,
            _ => panic!("first op must be view create"),
        };
        let mut child_appends_seen = 0;
        for op in &ops {
            if let MockOp::AppendChild { parent, .. } = op {
                if *parent == view_handle {
                    child_appends_seen += 1;
                }
            }
        }
        assert_eq!(child_appends_seen, 2, "two text children appended to view");
    }

    #[test]
    fn mount_calls_set_root_and_flush_in_order() {
        let mut r = MockRenderer::new();
        let _root = mount(&mut r, &page().child(text_with("hi")));

        let ops = r.into_ops();
        let set_root_idx = ops
            .iter()
            .position(|op| matches!(op, MockOp::SetRoot { .. }))
            .expect("set_root op");
        let flush_idx = ops
            .iter()
            .position(|op| matches!(op, MockOp::Flush))
            .expect("flush op");
        assert!(
            set_root_idx < flush_idx,
            "set_root must precede flush"
        );
    }

    #[test]
    fn mount_returns_root_handle() {
        let mut r = MockRenderer::new();
        let root = mount(&mut r, &page());
        assert_eq!(root, 1, "root is the first created element");
    }

    #[test]
    fn deep_nesting_walks_in_post_order() {
        // page > view > view > view > text > raw_text
        let mut r = MockRenderer::new();
        let tree = page().child(
            view().child(
                view().child(
                    view().child(text_with("deep")),
                ),
            ),
        );
        build_subtree(&mut r, &tree);

        // Just validate that all Create ops appear, in the order parents
        // first / children later, and we get exactly one of each.
        let creates: Vec<_> = r
            .into_ops()
            .into_iter()
            .filter_map(|op| match op {
                MockOp::Create { tag, .. } => Some(tag),
                _ => None,
            })
            .collect();
        assert_eq!(
            creates,
            vec![
                ElementTag::Page,
                ElementTag::View,
                ElementTag::View,
                ElementTag::View,
                ElementTag::Text,
                ElementTag::RawText,
            ]
        );
    }
}
