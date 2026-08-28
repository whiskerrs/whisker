//! A stable 100,000-row workload for profiling the Rust-owned List pipeline.

use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;

const ITEM_COUNT: u32 = 100_000;
const ROW_HEIGHT: i32 = 72;

#[component]
fn benchmark_row(row: Signal<u32>) -> Element {
    let title = computed(move || format!("Transaction #{:06}", row.get()));
    let subtitle = computed(move || format!("Stable key {} · recycled Rust slot", row.get()));
    let amount = computed(move || {
        let row = row.get();
        format!("+${}.{:02}", row % 10_000, row % 100)
    });

    render! {
        view(style: css!(
            width: percent(100),
            height: px(ROW_HEIGHT),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            padding_top: px(8),
            padding_right: px(16),
            padding_bottom: px(8),
            padding_left: px(16),
            background_color: Color::hex(0x111827),
        )) {
            text(
                style: css!(
                    color: Color::hex(0xF8FAFC),
                    font_size: px(15),
                    font_weight: FontWeight::Bold,
                ),
                value: title,
            )
            text(
                style: css!(color: Color::hex(0x94A3B8), font_size: px(12)),
                value: subtitle,
            )
            text(
                style: css!(color: Color::hex(0x22C55E), font_size: px(12)),
                value: amount,
            )
        }
    }
}

#[whisker::main]
pub fn app() -> Element {
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x09090B),
        )) {
            view(style: css!(
                width: percent(100),
                height: px(72),
                flex_shrink: 0.0,
                padding: px(16),
                background_color: Color::hex(0x18181B),
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xFAFAFA),
                        font_size: px(18),
                        font_weight: FontWeight::Bold,
                    ),
                    value: "Whisker List · 100,000 rows",
                )
                text(
                    style: css!(color: Color::hex(0xA1A1AA), font_size: px(12)),
                    value: "Profile a release build while flinging in both directions",
                )
            }
            list(
                style: css!(flex_grow: 1.0, width: percent(100)),
                each: || (0..ITEM_COUNT).collect::<Vec<_>>(),
                meta: |row: &u32| ItemMeta::key(*row)
                    .estimated_size(ROW_HEIGHT)
                    .reuse_identifier("transaction-row"),
                recycled_children: |row: ReadSignal<u32>| render! { benchmark_row(row: row) },
            )
        }
    }
}
