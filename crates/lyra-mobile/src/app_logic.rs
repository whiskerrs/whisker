//! User-app logic shared between the production FFI entry and integration
//! tests against `MockRenderer`.
//!
//! When `#[lyra::main]` lands (Phase C), the user's own `app()` will
//! replace this module entirely.

use lyra_macros::rsx;
use lyra_runtime::element::Element;
use lyra_runtime::signal::Signal;

/// Produce the element tree for one frame given the current `greeting`.
pub fn build_demo_tree(greeting: &str) -> Element {
    let greeting = greeting.to_owned();
    rsx! {
        page {
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; justify-content: center; align-items: center;",
            text {
                style: "font-size: 32px; color: black;",
                { greeting }
            }
        }
    }
}

/// Called by the runtime on every tick. Increments the demo counter so
/// the rendered text changes each frame, proving the reactive plumbing
/// (signal -> dirty -> diff -> apply -> bridge) actually closes the loop.
pub fn mutate_demo_state(tick_count: Signal<i32>) {
    tick_count.update(|n| n + 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyra_runtime::render::mount;
    use lyra_runtime::renderer::{MockOp, MockRenderer};
    use lyra_runtime::signal::{__reset_runtime, use_signal};

    #[test]
    fn demo_tree_has_expected_shape() {
        let tree = build_demo_tree("Hi");
        assert_eq!(tree.tag, lyra_runtime::element::ElementTag::Page);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(
            tree.children[0].tag,
            lyra_runtime::element::ElementTag::Text
        );
        assert_eq!(tree.children[0].children[0].get_attr("text"), Some("Hi"));
    }

    #[test]
    fn demo_tree_carries_through_styles() {
        let tree = build_demo_tree("anything");
        assert!(tree.styles.contains("background-color: white"));
        assert!(tree.children[0].styles.contains("font-size: 32px"));
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
        assert_eq!(create_count, 3); // page + text + raw_text

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

    #[test]
    fn mutate_demo_state_increments_signal() {
        __reset_runtime();
        let counter = use_signal(|| 0_i32);
        mutate_demo_state(counter);
        assert_eq!(counter.get(), 1);
        mutate_demo_state(counter);
        mutate_demo_state(counter);
        assert_eq!(counter.get(), 3);
    }
}
