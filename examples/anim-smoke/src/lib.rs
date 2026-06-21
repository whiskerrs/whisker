//! anim-smoke — on-device check of the whisker-animation engine.
//!
//! Two rows, one Toggle:
//!
//! - **Top (curve):** a purple box slides 240px on an `ease_out`, 600ms
//!   *time-driven* curve — progress is a pure function of elapsed time.
//! - **Bottom (spring):** a teal box slides the same 240px on a
//!   `bouncy()` *physics* spring — it overshoots and settles, with no
//!   fixed duration. Same Toggle drives both, side by side, so the
//!   curve and the spring character are directly comparable on-device.
//!
//! The point is to confirm the continuous engine drives smooth,
//! per-frame transforms for *both* timing strategies (forward/reverse,
//! idle when done).

use whisker::css::{AlignItems, Color, Display, FlexDirection, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker::{AnimConfig, animated};

#[whisker::main]
fn app() -> Element {
    render! {
        Root
    }
}

#[component]
fn root() -> Element {
    // Curve box: x animates 0 -> 240 over 600ms (ease-out).
    let (curve_x, curve_ctrl) = animated(0.0_f32, 240.0_f32, AnimConfig::ease_out(600));
    // Spring box: same 0 -> 240 travel, but a bouncy physics spring —
    // visible overshoot, no fixed duration.
    let (spring_x, spring_ctrl) = animated(0.0_f32, 240.0_f32, AnimConfig::bouncy());
    // Which end we're heading toward; flips on each tap.
    let forward = signal(false);

    render! {
        view(style: css!(
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            gap: px(40),
            background_color: Color::hex(0x0B0B0F),
        )) {
            // Row 1 — curve box.
            view(style: css!(
                width: px(300),
                height: px(64),
                border_radius: px(12),
                background_color: Color::hex(0x16161D),
                display: Display::Flex,
                align_items: AlignItems::Center,
            )) {
                view(style: computed(move || css!(
                    width: px(56),
                    height: px(56),
                    margin_left: px(4),
                    border_radius: px(10),
                    background_color: Color::hex(0x7C5CFF),
                )
                .raw("transform", format!("translateX({}px)", curve_x.get()))))
            }

            // Row 2 — spring box.
            view(style: css!(
                width: px(300),
                height: px(64),
                border_radius: px(12),
                background_color: Color::hex(0x16161D),
                display: Display::Flex,
                align_items: AlignItems::Center,
            )) {
                view(style: computed(move || css!(
                    width: px(56),
                    height: px(56),
                    margin_left: px(4),
                    border_radius: px(10),
                    background_color: Color::hex(0x29D6C5),
                )
                .raw("transform", format!("translateX({}px)", spring_x.get()))))
            }

            // Toggle button: flip direction and drive BOTH controllers.
            view(
                style: css!(
                    padding: (px(12), px(28)),
                    border_radius: px(12),
                    background_color: Color::hex(0x7C5CFF),
                ),
                on_tap: move |_| {
                    let next = !forward.get();
                    forward.set(next);
                    if next {
                        curve_ctrl.forward();
                        spring_ctrl.forward();
                    } else {
                        curve_ctrl.reverse();
                        spring_ctrl.reverse();
                    }
                },
            ) {
                text(
                    value: "Toggle",
                    style: css!(color: Color::hex(0xFFFFFF), font_size: px(18)),
                )
            }
        }
    }
}
