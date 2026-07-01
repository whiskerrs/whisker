//! Smoke test for whisker's `<list>` (on-demand virtualization + Option E).
//!
//! - A **full-span header** as item 0 (`list_item(full_span, estimated_size)`)
//!   — verifies bugs ② (header crush) / ③ (cross-axis width).
//! - **Variable-height rows** — verifies uniform width + recycling under scroll.
//! - **Rotate / Prepend** buttons mutate the data order — verifies bug ①
//!   (stable item-key reorders correctly instead of appending at the tail).

use whisker::css::{AlignItems, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[derive(Clone)]
enum Row {
    Header,
    Item(u32),
}

impl Row {
    fn key(&self) -> String {
        match self {
            Row::Header => "header".to_string(),
            Row::Item(n) => format!("item-{n}"),
        }
    }
}

/// Variable-length body → variable row height.
fn body_text(n: u32) -> String {
    "lorem ipsum dolor sit amet ".repeat(((n % 4) + 1) as usize)
}

#[whisker::main]
pub fn app() -> Element {
    let ids = signal((1u32..=30).collect::<Vec<u32>>());
    let next = signal(100u32);

    let rotate = move |_| {
        let mut v = ids.get();
        if !v.is_empty() {
            v.rotate_left(1);
        }
        ids.set(v);
    };
    let prepend = move |_| {
        let n = next.get();
        next.set(n + 1);
        let mut v = ids.get();
        v.insert(0, n);
        ids.set(v);
    };

    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x101012),
            padding_top: px(48),
        )) {
            view(style: css!(flex_direction: FlexDirection::Row, padding: px(12))) {
                view(
                    style: css!(
                        background_color: Color::hex(0x2563EB),
                        padding: px(12),
                        margin_right: px(12),
                        border_radius: px(8),
                    ),
                    on_tap: rotate,
                ) {
                    text(style: css!(color: Color::hex(0xFFFFFF), font_weight: FontWeight::Bold), value: "Rotate")
                }
                view(
                    style: css!(
                        background_color: Color::hex(0x16A34A),
                        padding: px(12),
                        border_radius: px(8),
                    ),
                    on_tap: prepend,
                ) {
                    text(style: css!(color: Color::hex(0xFFFFFF), font_weight: FontWeight::Bold), value: "Prepend +")
                }
            }
            list(
                style: css!(flex_grow: 1.0, width: percent(100)),
                each: move || {
                    let mut rows = vec![Row::Header];
                    rows.extend(ids.get().into_iter().map(Row::Item));
                    rows
                },
                key: |r: &Row| r.key(),
                children: |r: Row| match r {
                    Row::Header => render! {
                        list_item(full_span: true, estimated_size: 160, reuse_identifier: "header") {
                            view(style: css!(
                                width: percent(100),
                                height: px(160),
                                background_color: Color::hex(0x2563EB),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                            )) {
                                text(
                                    style: css!(color: Color::hex(0xFFFFFF), font_size: px(20), font_weight: FontWeight::Bold),
                                    value: "FULL-SPAN HEADER (item 0)",
                                )
                            }
                        }
                    },
                    Row::Item(n) => render! {
                        list_item(reuse_identifier: "row", estimated_size: 84) {
                            view(style: css!(
                                width: percent(100),
                                padding: px(16),
                                flex_direction: FlexDirection::Column,
                                background_color: Color::hex(0x18181B),
                                margin_bottom: px(1),
                            )) {
                                text(
                                    style: css!(color: Color::hex(0xF5F5F7), font_size: px(16), font_weight: FontWeight::Bold),
                                    value: format!("Row {n}"),
                                )
                                text(
                                    style: css!(color: Color::hex(0x9AA0AA), font_size: px(13)),
                                    value: body_text(n),
                                )
                            }
                        }
                    },
                },
            )
        }
    }
}
