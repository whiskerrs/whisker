//! Smoke test for whisker's `<list>` (on-demand virtualization + Option E).
//!
//! - A **full-span header** as item 0
//!   — verifies bugs ② (header crush) / ③ (cross-axis width).
//! - **Variable-height rows** — verifies uniform width + recycling under scroll.
//! - **Rotate / Prepend** buttons mutate the data order — verifies bug ①
//!   (stable item-key reorders correctly instead of appending at the tail).
//!
//! # Self-driving scenarios
//!
//! Synthetic touches do not exercise the physical Host scroll pipeline,
//! so the smoke drives itself with programmatic scrolls chained off the
//! `layoutcomplete` / `scrollstatechange` events. Pick a scenario with
//! `SIMCTL_CHILD_SMOKE_SCENARIO=<name> xcrun simctl launch …`:
//!
//! | scenario  | data    | drives                                            |
//! |-----------|---------|---------------------------------------------------|
//! | (unset)   | late    | scroll to bottom → append page (position holds) → back to top (row 1 re-materializes) |
//! | `fill`    | late    | nothing — observe the no-interaction viewport fill |
//! | `prepend` | late    | scroll to mid → prepend 3 rows → position must stay anchored |
//! | `remove`  | late    | scroll to mid → remove 5 rows above the viewport → anchored |
//! | `upper`   | late    | scroll to bottom → back toward top → `scrolltoupper` fires |
//! | `sticky`  | mounted | `sticky` list + `sticky_top` header → scroll to mid → header stays pinned |
//! | `initial` | mounted | `initial_scroll_index: 15` → launches mid-list     |
//! | `waterfall` | mounted | `list_type: waterfall` + `span_count: 2` — 2-column staggered layout |

use whisker::ListHandle;
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

fn scenario() -> &'static str {
    // Leaked once — the scenario is fixed for the process lifetime.
    static SCENARIO: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SCENARIO.get_or_init(|| std::env::var("SMOKE_SCENARIO").unwrap_or_default())
}

#[whisker::main]
pub fn app() -> Element {
    let scen = scenario();
    // Late data (populated in `on_mount`, AFTER the first layout)
    // exercises the late-data fill path. `sticky` / `initial` /
    // `waterfall` need the data present at mount instead: their list
    // attributes anchor the FIRST layout.
    let late_data = !matches!(scen, "sticky" | "initial" | "waterfall");
    let ids = signal(if late_data {
        Vec::<u32>::new()
    } else {
        (1u32..=30).collect::<Vec<u32>>()
    });
    let next = signal(100u32);
    if late_data {
        on_mount(move || ids.set((1u32..=30).collect::<Vec<u32>>()));
    }
    let list_handle = ListHandle::<String>::new();

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

    // Default scenario: append a page when the List enters its end threshold.
    let on_lower = move || {
        eprintln!("[SMOKE] end threshold reached");
        if !scenario().is_empty() {
            return;
        }
        let mut v = ids.get();
        if v.len() >= 120 {
            return; // cap the smoke run
        }
        let n = next.get();
        next.set(n + 10);
        v.extend(n..n + 10);
        ids.set(v);
    };

    let on_upper = || {
        eprintln!("[SMOKE] start threshold reached");
    };

    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x101012),
            padding_top: px(48),
        )) {
            view(style: css!(flex_direction: FlexDirection::Row, padding: px(12), flex_shrink: 0.0)) {
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
                start_reached_threshold: 88.0,
                end_reached_threshold: 88.0,
                on_start_reached: on_upper,
                on_end_reached: on_lower,
                initial_scroll: if scen == "initial" {
                    ListScrollTarget::index(15, ScrollAlignment::Start)
                } else {
                    ListScrollTarget::start()
                },
                ref: list_handle.r(),
                on_scroll: |e| eprintln!("[SMOKE] scroll fired: {e:?}"),
                each: move || {
                    let mut rows = vec![Row::Header];
                    rows.extend(ids.get().into_iter().map(Row::Item));
                    rows
                },
                key: |r: &Row| r.key(),
                children: |r: ReadSignal<Row>| match r.get() {
                    Row::Header => render! {
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
                    },
                    Row::Item(n) => render! {
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
                    },
                },
            )
        }
    }
}
