//! User-app logic shared between the production FFI entry and integration
//! tests against `MockRenderer`.
//!
//! Phase A3 demo: a tap-driven counter. The Rust side owns the count
//! signal; tap on the text element fires a Lynx event that flows through
//! the bridge into the Rust closure, which updates the signal. The
//! runtime detects the dirty flag on the next frame and re-renders.

use lyra_macros::rsx;
use lyra_runtime::element::Element;
use lyra_runtime::signal::Signal;

/// Build the demo tree given the current count and the closure to fire
/// on tap.
pub fn build_counter_tree(count: i32, on_tap: impl Fn() + Send + Sync + 'static) -> Element {
    rsx! {
        page {
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column; \
                    justify-content: center; align-items: center;",
            text {
                style: "font-size: 48px; color: black; margin-bottom: 16px;",
                { format!("Count: {}", count) }
            }
            text {
                style: "font-size: 20px; color: blue; padding: 12px 24px; \
                        background-color: #eef; border-radius: 8px;",
                on_tap: on_tap,
                "Tap to increment"
            }
        }
    }
}

/// Wire `count` so its current value is read by the renderer and so that
/// invoking the returned closure increments it. Both pieces are needed
/// for the reactive round-trip; bundling them here keeps the pairing
/// in one place.
pub fn counter_render_tree(count: Signal<i32>) -> Element {
    let on_tap = move || {
        count.update(|n| n + 1);
    };
    build_counter_tree(count.get(), on_tap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyra_runtime::render::mount;
    use lyra_runtime::renderer::{MockOp, MockRenderer};
    use lyra_runtime::runtime::Runtime;
    use lyra_runtime::signal::{__reset_runtime, use_signal};

    #[test]
    fn counter_tree_has_two_text_children() {
        let tree = build_counter_tree(0, || {});
        assert_eq!(tree.children.len(), 2);
    }

    #[test]
    fn counter_tree_renders_count_value() {
        let tree = build_counter_tree(42, || {});
        // Count text is the first child > raw_text
        assert_eq!(tree.children[0].children[0].get_attr("text"), Some("Count: 42"));
    }

    #[test]
    fn counter_tree_attaches_tap_handler_to_button() {
        let tree = build_counter_tree(0, || {});
        let button = &tree.children[1];
        assert_eq!(button.events.len(), 1);
        assert_eq!(button.events[0].name, "tap");
    }

    #[test]
    fn mounted_counter_records_event_listener_op() {
        __reset_runtime();
        let mut r = MockRenderer::new();
        let _ = mount(&mut r, &build_counter_tree(0, || {}));
        let listener_op = r
            .ops()
            .iter()
            .find(|op| matches!(op, MockOp::SetEventListener { event_name, .. } if event_name == "tap"));
        assert!(listener_op.is_some());
    }

    #[test]
    fn firing_tap_through_runtime_updates_count_text() {
        __reset_runtime();
        let count = use_signal(|| 0_i32);
        let app = move || counter_render_tree(count);
        let mut rt = Runtime::new(MockRenderer::new(), app);

        // Find the handle of the tap-bound element from the recorded ops.
        let tap_handle = rt
            .renderer()
            .ops()
            .iter()
            .find_map(|op| match op {
                MockOp::SetEventListener { handle, event_name } if event_name == "tap" => {
                    Some(*handle)
                }
                _ => None,
            })
            .expect("event listener registered during mount");

        let before_ops = rt.renderer().ops().len();
        // Simulate a tap.
        assert!(rt.renderer().fire_event(tap_handle, "tap"));
        // The tap synchronously updated the signal; the runtime's next
        // frame must produce a SetAttribute on the count text.
        rt.frame();

        let new_ops = &rt.renderer().ops()[before_ops..];
        let updated = new_ops.iter().find_map(|op| match op {
            MockOp::SetAttribute { key, value, .. } if key == "text" && value.starts_with("Count:") => {
                Some(value.clone())
            }
            _ => None,
        });
        assert_eq!(updated.as_deref(), Some("Count: 1"));
    }

    #[test]
    fn many_taps_increment_count_each_frame() {
        __reset_runtime();
        let count = use_signal(|| 0_i32);
        let app = move || counter_render_tree(count);
        let mut rt = Runtime::new(MockRenderer::new(), app);

        let tap_handle = rt
            .renderer()
            .ops()
            .iter()
            .find_map(|op| match op {
                MockOp::SetEventListener { handle, event_name } if event_name == "tap" => {
                    Some(*handle)
                }
                _ => None,
            })
            .unwrap();

        for _ in 0..5 {
            rt.renderer().fire_event(tap_handle, "tap");
            rt.frame();
        }

        let last = rt
            .renderer()
            .ops()
            .iter()
            .rev()
            .find_map(|op| match op {
                MockOp::SetAttribute { key, value, .. } if key == "text" && value.starts_with("Count:") => {
                    Some(value.clone())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(last, "Count: 5");
    }
}
