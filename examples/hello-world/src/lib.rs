//! Hello World — the demo Lyra app the iOS sample binds.
//!
//! Annotated with `#[lyra::main]`; the macro expands to the
//! `lyra_mobile_app_main` / `lyra_mobile_tick` C ABI exports the host
//! `LyraView.swift` calls into, plus the runtime/signal plumbing the
//! framework needs.

use lyra::prelude::*;

/// Module-scope tick counter. Every `lyra_mobile_tick` from the host
/// bumps this, and because `app()` reads it via `Signal::get()` the
/// runtime knows to re-render. When tap-driven events flow on iOS, the
/// "Tap to increment" `on_tap:` handler will drive the same signal —
/// the app function doesn't have to change.
fn counter() -> Signal<i32> {
    thread_local! {
        static COUNT: std::cell::OnceCell<Signal<i32>> = const { std::cell::OnceCell::new() };
    }
    COUNT.with(|cell| *cell.get_or_init(|| use_signal(|| 0_i32)))
}

#[lyra::main]
fn app() -> Element {
    let count = counter();
    let on_tap = move || count.update(|n| n + 1);
    rsx! {
        page {
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column; \
                    justify-content: center; align-items: center;",
            text {
                style: "font-size: 48px; color: black; margin-bottom: 16px;",
                { format!("Count: {}", count.get()) }
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

/// Visible to the host so it can drive ticks from a Swift `Timer` while
/// the proper event-driven path is still being unblocked. Once tap
/// events flow this can go away.
#[no_mangle]
pub extern "C" fn hello_world_tick_signal() {
    counter().update(|n| n + 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lyra::runtime::renderer::{MockOp, MockRenderer};
    use lyra::runtime::render::mount;

    #[test]
    fn app_returns_a_page() {
        // Reset thread-locals between tests so each starts at count = 0.
        lyra::runtime::signal::__reset_runtime();
        let tree = app();
        assert_eq!(tree.tag, ElementTag::Page);
        assert_eq!(tree.children.len(), 2);
    }

    #[test]
    fn count_text_starts_at_zero() {
        lyra::runtime::signal::__reset_runtime();
        let tree = app();
        assert_eq!(
            tree.children[0].children[0].get_attr("text"),
            Some("Count: 0"),
        );
    }

    #[test]
    fn mounts_with_two_text_create_ops() {
        lyra::runtime::signal::__reset_runtime();
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
        // page + text + raw_text + text + raw_text = 5 elements
        assert_eq!(creates.len(), 5);
    }
}
