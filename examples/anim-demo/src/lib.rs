//! Lynx `element.animate()` smoke test.
//!
//! Calls `animate_start` against the blue box via the new
//! `lynx_element_animate` bridge. If the bridge + Lynx native
//! animator are wired correctly the box slides from the left to
//! 250px on the right over 1 second.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::{animate_start, AnimateOptions, ElementHandle};

#[whisker::main]
pub fn render_app() -> Element {
    let box_handle = ElementHandle::new();
    let trigger_handle = box_handle;

    // Mount-time auto-animate: as soon as the box mounts, kick off a
    // slide. Re-runnable via the button below.
    on_mount(move || {
        run_slide(box_handle);
    });

    render! {
        page(
            style: "width: 100vw; height: 100vh; background-color: white; \
                    display: flex; flex-direction: column; gap: 24px; padding: 48px 16px;",
        ) {
            text(
                style: "font-size: 18px; font-family: monospace;",
                value: "lynx_element_animate smoke test",
            )

            view(style: "width: 100%; height: 80px; background-color: #f3f4f6; border-radius: 12px; position: relative;") {
                view(
                    ref: box_handle.r(),
                    style: "width: 80px; height: 80px; background-color: #3b82f6; \
                            border-radius: 12px;",
                )
            }

            view(
                style: "padding: 12px; background-color: #1d4ed8; border-radius: 8px;",
                on_tap: move |_| run_slide(trigger_handle),
            ) {
                text(
                    style: "color: white; font-weight: 700;",
                    value: "Re-run slide",
                )
            }
        }
    }
}

fn run_slide(handle: ElementHandle) {
    let Some(el) = handle.r().element() else {
        return;
    };
    let result = animate_start(
        el,
        "slide",
        &[
            ("0%", &[("transform", "translateX(0px)")]),
            ("100%", &[("transform", "translateX(250px)")]),
        ],
        &AnimateOptions {
            duration_ms: 1000,
            easing: "ease-in-out".into(),
            fill: "forwards".into(),
            ..Default::default()
        },
    );
    if let Err(e) = result {
        eprintln!("animate_start failed: {e}");
    }
}
