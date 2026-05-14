//! Walk an [`Element`] tree and emit creation / mutation ops to a
//! [`Renderer`]. Returns a [`HandleTree`] that mirrors the element tree
//! so the diff engine can resolve `(prev_tree, path)` -> renderer handle
//! in O(path-depth).

use crate::element::Element;
use crate::renderer::Renderer;

/// A tree of renderer handles that mirrors the [`Element`] tree it was
/// built from. The diff engine walks this in lockstep with patches to
/// look up handles by `Path`.
#[derive(Debug, Clone)]
pub struct HandleTree<H> {
    pub handle: H,
    pub children: Vec<HandleTree<H>>,
}

impl<H: Copy> HandleTree<H> {
    /// Look up the handle for the node at `path`.
    pub fn at_path(&self, path: &[usize]) -> Option<H> {
        let mut node = self;
        for &i in path {
            node = node.children.get(i)?;
        }
        Some(node.handle)
    }

    /// Borrow the [`HandleTree`] subtree at `path`.
    pub fn subtree(&self, path: &[usize]) -> Option<&HandleTree<H>> {
        let mut node = self;
        for &i in path {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// Mutably borrow the subtree at `path`.
    pub fn subtree_mut(&mut self, path: &[usize]) -> Option<&mut HandleTree<H>> {
        let mut node = self;
        for &i in path {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }
}

/// Mount `tree` as the engine's root and flush a frame, returning the
/// [`HandleTree`] mirror so subsequent diff/apply cycles can locate
/// handles by path.
pub fn mount<R: Renderer>(renderer: &mut R, tree: &Element) -> HandleTree<R::ElementHandle> {
    let handle_tree = build_subtree(renderer, tree);
    renderer.set_root(handle_tree.handle);
    renderer.flush();
    handle_tree
}

/// Materialize an Element subtree on the renderer and return a
/// [`HandleTree`] mirroring it. Does NOT call `set_root` or `flush` —
/// meant to be composed.
pub fn build_subtree<R: Renderer>(
    renderer: &mut R,
    node: &Element,
) -> HandleTree<R::ElementHandle> {
    let handle = renderer.create_element(node.tag);

    if !node.styles.is_empty() {
        renderer.set_inline_styles(handle, &node.styles);
    }
    for attr in &node.attrs {
        renderer.set_attribute(handle, &attr.name, &attr.value);
    }
    for event in &node.events {
        let cb = event.callback.clone();
        renderer.set_event_listener(handle, &event.name, Box::new(move || cb()));
    }

    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        let child_tree = build_subtree(renderer, child);
        renderer.append_child(handle, child_tree.handle);
        children.push(child_tree);
    }

    HandleTree { handle, children }
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
        let tree = build_subtree(&mut r, &view());
        assert_eq!(tree.handle, 1);
        assert!(tree.children.is_empty());
        assert_eq!(
            r.into_ops(),
            vec![MockOp::Create {
                handle: 1,
                tag: ElementTag::View
            }]
        );
    }

    #[test]
    fn handle_tree_mirrors_element_tree() {
        let mut r = MockRenderer::new();
        let tree = build_subtree(&mut r, &page().child(view().child(text_with("hi"))));
        // page (1) -> view (2) -> text (3) -> raw_text (4)
        assert_eq!(tree.handle, 1);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].handle, 2);
        assert_eq!(tree.children[0].children[0].handle, 3);
        assert_eq!(tree.children[0].children[0].children[0].handle, 4);
    }

    #[test]
    fn at_path_returns_handle_for_root() {
        let mut r = MockRenderer::new();
        let tree = build_subtree(&mut r, &view().child(text()));
        assert_eq!(tree.at_path(&[]), Some(1));
        assert_eq!(tree.at_path(&[0]), Some(2));
    }

    #[test]
    fn at_path_returns_none_for_oob() {
        let mut r = MockRenderer::new();
        let tree = build_subtree(&mut r, &view());
        assert_eq!(tree.at_path(&[0]), None);
        assert_eq!(tree.at_path(&[3, 7]), None);
    }

    #[test]
    fn at_path_walks_deeply() {
        let mut r = MockRenderer::new();
        let tree = build_subtree(
            &mut r,
            &page()
                .child(view().child(text_with("a")))
                .child(view().child(text_with("b"))),
        );
        // Layout:
        // page(1) -> view(2)[ text(3)[ raw(4) ] ], view(5)[ text(6)[ raw(7) ] ]
        assert_eq!(tree.at_path(&[1, 0, 0]), Some(7));
    }

    #[test]
    fn styles_emitted_when_nonempty() {
        let mut r = MockRenderer::new();
        build_subtree(&mut r, &view().style("color: red"));
        assert_eq!(
            r.into_ops(),
            vec![
                MockOp::Create {
                    handle: 1,
                    tag: ElementTag::View
                },
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
    fn mount_calls_set_root_and_flush_in_order() {
        let mut r = MockRenderer::new();
        let _ = mount(&mut r, &page().child(text_with("hi")));

        let ops = r.into_ops();
        let set_root_idx = ops
            .iter()
            .position(|op| matches!(op, MockOp::SetRoot { .. }))
            .expect("set_root op");
        let flush_idx = ops
            .iter()
            .position(|op| matches!(op, MockOp::Flush))
            .expect("flush op");
        assert!(set_root_idx < flush_idx);
    }

    #[test]
    fn mount_returns_handle_tree_with_root_at_path_empty() {
        let mut r = MockRenderer::new();
        let tree = mount(&mut r, &page());
        assert_eq!(tree.at_path(&[]), Some(1));
    }
}
