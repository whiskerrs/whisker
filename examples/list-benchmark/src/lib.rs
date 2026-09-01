//! A stable 100,000-cell Grid workload for profiling the Rust-owned List pipeline.

use whisker::css::{FlexDirection, FontWeight, GridTemplate, GridTrack};
use whisker::prelude::*;
use whisker::runtime::view::Element;

const ITEM_COUNT: u32 = 100_000;
const COLUMN_COUNT: u32 = 2;
const GRID_ROW_COUNT: u32 = ITEM_COUNT / COLUMN_COUNT;
const GRID_ROW_HEIGHT: i32 = 132;

#[component]
fn benchmark_card(item: Signal<u32>) -> Element {
    let title = computed(move || format!("Transaction #{:06}", item.get()));
    let subtitle = computed(move || format!("Stable key {} · recycled slot", item.get()));
    let amount = computed(move || {
        let item = item.get();
        format!("+${}.{:02}", item % 10_000, item % 100)
    });

    render! {
        View(style: css!(
            width: percent(100),
            height: px(116),
            flex_direction: FlexDirection::Column,
            padding: px(12),
            border_radius: px(12),
            background_color: Color::hex(0x111827),
        )) {
            Text(
                style: css!(
                    color: Color::hex(0xF8FAFC),
                    font_size: px(15),
                    font_weight: FontWeight::Bold,
                ),
                value: title,
            )
            Text(
                style: css!(color: Color::hex(0x94A3B8), font_size: px(12)),
                value: subtitle,
            )
            Text(
                style: css!(color: Color::hex(0x22C55E), font_size: px(12)),
                value: amount,
            )
        }
    }
}

#[component]
fn benchmark_grid_row(row: Signal<u32>) -> Element {
    let first = computed(move || row.get() * COLUMN_COUNT);
    let second = computed(move || row.get() * COLUMN_COUNT + 1);

    render! {
        View(style: Css::new()
            .display_grid()
            .width(percent(100))
            .height(px(GRID_ROW_HEIGHT))
            .grid_template_columns(GridTemplate::tracks([
                GridTrack::fraction(1.0),
                GridTrack::fraction(1.0),
            ]))
            .column_gap(px(12))
            .padding_top(px(8))
            .padding_right(px(12))
            .padding_bottom(px(8))
            .padding_left(px(12))
        ) {
            BenchmarkCard(item: first)
            BenchmarkCard(item: second)
        }
    }
}

#[whisker::main]
pub fn app() -> Element {
    render! {
        View(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x09090B),
        )) {
            View(style: css!(
                width: percent(100),
                height: px(104),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding_top: px(36),
                padding_right: px(16),
                padding_bottom: px(12),
                padding_left: px(16),
                background_color: Color::hex(0x18181B),
            )) {
                Text(
                    style: css!(
                        color: Color::hex(0xFAFAFA),
                        font_size: px(18),
                        font_weight: FontWeight::Bold,
                    ),
                    value: "Whisker List · 100,000 cells",
                )
                Text(
                    style: css!(color: Color::hex(0xA1A1AA), font_size: px(12)),
                    value: "2-column CSS Grid · Rust-owned virtualized rows",
                )
            }
            List(
                style: css!(flex_grow: 1.0, width: percent(100)),
                each: || (0..GRID_ROW_COUNT).collect::<Vec<_>>(),
                key: |row: &u32| *row,
                children: |row: ReadSignal<u32>| render! {
                    BenchmarkGridRow(row: row)
                },
            )
        }
    }
}
