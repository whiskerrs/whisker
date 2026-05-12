//! User-app logic shared between the production FFI entry and integration
//! tests against `MockRenderer`.
//!
//! This is currently the canned example tree (Phase 8 demo). When the
//! `#[lyra::main]` attribute is fleshed out, the user's own `app()`
//! function will replace this.

use lyra_runtime::element::Element;
use lyra_runtime::prelude::*;

/// Returns the static (no-state) Lyra example tree the iOS demo renders.
pub fn build_demo_tree(greeting: &str) -> Element {
    page()
        .style(
            "width: 100vw; height: 100vh; background-color: white; \
             display: flex; justify-content: center; align-items: center;",
        )
        .child(
            text()
                .style("font-size: 32px; color: black;")
                .child(raw_text(greeting)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyra_runtime::renderer::{MockOp, MockRenderer};
    use lyra_runtime::render::mount;

    #[test]
    fn demo_tree_has_expected_shape() {
        let tree = build_demo_tree("Hi");
        assert_eq!(tree.tag, lyra_runtime::element::ElementTag::Page);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].tag, lyra_runtime::element::ElementTag::Text);
        assert_eq!(
            tree.children[0].children[0].get_attr("text"),
            Some("Hi"),
        );
    }

    #[test]
    fn mounts_with_expected_renderer_ops() {
        let mut r = MockRenderer::new();
        let _ = mount(&mut r, &build_demo_tree("Hello"));

        let create_count = r
            .ops()
            .iter()
            .filter(|op| matches!(op, MockOp::Create { .. }))
            .count();
        // page + text + raw_text = 3 elements.
        assert_eq!(create_count, 3);

        // The greeting must travel through to a SetAttribute("text", ...).
        let text_attr = r.ops().iter().find_map(|op| match op {
            MockOp::SetAttribute { key, value, .. } if key == "text" => Some(value.clone()),
            _ => None,
        });
        assert_eq!(text_attr.as_deref(), Some("Hello"));
    }

    #[test]
    fn ends_with_set_root_then_flush() {
        let mut r = MockRenderer::new();
        let _ = mount(&mut r, &build_demo_tree("x"));
        let ops = r.ops();
        let last_two: Vec<_> = ops.iter().rev().take(2).collect();
        assert!(matches!(last_two[0], MockOp::Flush));
        assert!(matches!(last_two[1], MockOp::SetRoot { .. }));
    }
}
