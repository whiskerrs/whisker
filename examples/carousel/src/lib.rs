//! Horizontal ScrollView snap smoke test.

use whisker::attrs::ScrollSnapStop;
use whisker::css::{AlignItems, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

const CARD_WIDTH: f64 = 280.0;
const CARD_STRIDE: f64 = 296.0;
const CARD_COUNT: i32 = 6;

#[component]
fn carousel_card(index: i32, title: &'static str, body: &'static str, color: Color) -> Element {
    render! {
        view(style: css!(
            width: px(CARD_WIDTH as i32),
            height: px(300),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            margin_left: px(16),
            padding: px(24),
            border_radius: px(28),
            background_color: color,
        )) {
            view(style: css!(
                width: px(48),
                height: px(48),
                border_radius: percent(50),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                background_color: Color::rgba(255, 255, 255, 0.2),
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                        font_weight: FontWeight::Bold,
                    ),
                    value: format!("{:02}", index + 1),
                )
            }
            view(style: css!(flex_direction: FlexDirection::Column)) {
                text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(26),
                        font_weight: FontWeight::Bold,
                    ),
                    value: title,
                )
                text(
                    style: css!(
                        color: Color::rgba(255, 255, 255, 0.78),
                        font_size: px(14),
                        line_height: px(21),
                        margin_top: px(8),
                    ),
                    value: body,
                )
            }
        }
    }
}

#[component]
fn page_dot(index: i32, current: ReadSignal<i32>) -> Element {
    let style = computed(move || {
        let active = current.get() == index;
        css!(
            width: px(if active { 24 } else { 8 }),
            height: px(8),
            margin_right: px(6),
            border_radius: px(4),
            background_color: if active {
                Color::hex(0xF8FAFC)
            } else {
                Color::hex(0x475569)
            },
        )
    });
    render! { view(style: style) }
}

#[whisker::main]
pub fn app() -> Element {
    let current = signal(0_i32);
    let on_scroll = move |event: whisker::event::ScrollEvent| {
        let page = (event.detail.scroll_left / CARD_STRIDE)
            .round()
            .clamp(0.0, f64::from(CARD_COUNT - 1)) as i32;
        if current.get() != page {
            current.set(page);
        }
    };

    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x070B14),
            padding_top: px(48),
            padding_bottom: px(28),
        )) {
            view(style: css!(
                flex_direction: FlexDirection::Column,
                padding_left: px(20),
                padding_right: px(20),
                margin_bottom: px(24),
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xF8FAFC),
                        font_size: px(28),
                        font_weight: FontWeight::Bold,
                    ),
                    value: "Scroll snap carousel",
                )
                text(
                    style: css!(
                        color: Color::hex(0x94A3B8),
                        font_size: px(14),
                        margin_top: px(6),
                    ),
                    value: "Flick quickly: Always should advance exactly one card.",
                )
            }
            scroll_view(
                axis: ScrollAxis::Horizontal,
                snap: ScrollSnap::start().with_offset(-16.0),
                scroll_snap_stop: ScrollSnapStop::Always,
                on_scroll: on_scroll,
                style: css!(
                    width: percent(100),
                    height: px(300),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                ),
            ) {
                carousel_card(index: 0, title: "Aurora", body: "The first card verifies the leading-edge clamp.", color: Color::hex(0x7C3AED))
                carousel_card(index: 1, title: "Current", body: "A short drag should choose the nearest direct child.", color: Color::hex(0x2563EB))
                carousel_card(index: 2, title: "Momentum", body: "Even a fast fling must stop at the adjacent card.", color: Color::hex(0x0891B2))
                carousel_card(index: 3, title: "Native", body: "Each Host keeps its own scrolling physics and presentation.", color: Color::hex(0x059669))
                carousel_card(index: 4, title: "Shared", body: "The same typed ScrollView contract drives every Host.", color: Color::hex(0xD97706))
                carousel_card(index: 5, title: "Finish", body: "The last card verifies trailing-content clamping.", color: Color::hex(0xDC2626))
            }
            view(style: css!(
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                view(style: css!(flex_direction: FlexDirection::Row, align_items: AlignItems::Center)) {
                    page_dot(index: 0, current: current.read_only())
                    page_dot(index: 1, current: current.read_only())
                    page_dot(index: 2, current: current.read_only())
                    page_dot(index: 3, current: current.read_only())
                    page_dot(index: 4, current: current.read_only())
                    page_dot(index: 5, current: current.read_only())
                }
            }
        }
    }
}
