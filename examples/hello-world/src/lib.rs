//! Hello World — the demo Tuft app the iOS sample binds.
//!
//! Annotated with `#[tuft::main]`; the macro expands to the
//! `tuft_app_main` / `tuft_tick` C ABI exports the host `TuftView.swift`
//! calls into, plus the runtime/signal plumbing the framework needs.

use tuft::prelude::*;

fn counter() -> Signal<i32> {
    thread_local! {
        static COUNT: std::cell::OnceCell<Signal<i32>> = const { std::cell::OnceCell::new() };
    }
    COUNT.with(|cell| *cell.get_or_init(|| use_signal(|| 0_i32)))
}

fn build_row(i: usize) -> Element {
    text()
        .style(
            "font-size: 20px; padding: 16px 24px; color: black; \
             background-color: white; border-bottom-width: 1px; \
             border-bottom-color: #ddd; border-bottom-style: solid;",
        )
        .child(raw_text(format!("Row {i}")))
}

#[tuft::main]
fn app() -> Element {
    let count = counter();
    let on_tap = move || count.update(|n| n + 1);

    // 30 scrollable rows. rsx! has no spread syntax yet, so we build the
    // list with the builder API and attach it as the last child of page.
    let list = scroll_view()
        .attr("scroll-orientation", "vertical")
        .style(
            "width: 90%; height: 55%; background-color: #f5f5f5; \
             border-radius: 12px; margin-top: 8px;",
        )
        .children((1..=30).map(build_row));

    rsx! {
        page {
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column; \
                    align-items: center; padding-top: 60px;",
            text {
                style: "font-size: 32px; color: black; margin-bottom: 12px;",
                { format!("Count: {}", count.get()) }
            }
            text {
                style: "font-size: 18px; color: blue; padding: 10px 20px; \
                        background-color: #eef; border-radius: 8px; \
                        margin-bottom: 20px;",
                on_tap: on_tap,
                "Tap to increment"
            }
        }
    }
    .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuft::runtime::renderer::{MockOp, MockRenderer};
    use tuft::runtime::render::mount;

    #[test]
    fn app_returns_a_page_with_scroll_view() {
        // Reset thread-locals between tests so each starts at count = 0.
        tuft::runtime::signal::__reset_runtime();
        let tree = app();
        assert_eq!(tree.tag, ElementTag::Page);
        // count text + button text + scroll-view
        assert_eq!(tree.children.len(), 3);
        assert_eq!(tree.children[2].tag, ElementTag::ScrollView);
        assert_eq!(tree.children[2].children.len(), 30);
    }

    #[test]
    fn count_text_starts_at_zero() {
        tuft::runtime::signal::__reset_runtime();
        let tree = app();
        assert_eq!(
            tree.children[0].children[0].get_attr("text"),
            Some("Count: 0"),
        );
    }

    #[test]
    fn mounts_creates_expected_elements() {
        tuft::runtime::signal::__reset_runtime();
        let mut r = MockRenderer::new();
        mount(&mut r, &app());

        let creates: Vec<_> = r
            .ops()
            .iter()
            .filter_map(|op| match op {
                MockOp::Create { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        // page + (text + raw_text) + (text + raw_text) + scroll-view
        //   + 30 * (text + raw_text) = 66 elements.
        assert_eq!(creates.len(), 66);
    }
}
