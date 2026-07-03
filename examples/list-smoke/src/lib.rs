//! Smoke test for whisker's `<list>` (on-demand virtualization + Option E).
//!
//! - A **full-span header** as item 0 (`list_item(full_span, estimated_size)`)
//!   — verifies bugs ② (header crush) / ③ (cross-axis width).
//! - **Variable-height rows** — verifies uniform width + recycling under scroll.
//! - **Rotate / Prepend** buttons mutate the data order — verifies bug ①
//!   (stable item-key reorders correctly instead of appending at the tail).
//!
//! # Self-driving scenarios
//!
//! Synthetic touches don't reach Lynx's scroll pipeline on the simulator,
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
    let list_handle = ListHandle::new();
    // The layoutcomplete on which the full data is native-side and the
    // scenario's first scroll may run: with late data the FIRST
    // layoutcomplete is the pre-data (header-only) layout.
    let data_lc = if late_data { 2 } else { 1 };
    let lc_seen = signal(0u32);
    // Scenario step machine: 0 = waiting for data layout, 1 = step-1
    // scroll issued, 2 = mutation done (terminal).
    let step = signal(0u32);

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

    // Default scenario: append a page on scroll-to-bottom — the
    // infinite-scroll pattern. Insert-only diff ⇒ the scroll position
    // must HOLD at the bottom. At the cap, scroll back to the top once —
    // the first row must re-materialize after having been recycled.
    let back_to_top_done = signal(false);
    let chase_bottom = signal(false);
    let on_lower = move |e: whisker::event::ScrollEvent| {
        eprintln!("[SMOKE] scrolltolower fired: {e:?}");
        if scenario() != "" {
            return;
        }
        if e.detail.scroll_height < 500.0 {
            // The pre-data (header-only, 160px) list is trivially "at
            // the bottom" — ignore that spurious trigger. Gate on the
            // EVENT's own content height: the signal may already hold
            // the real data by the time this drains.
            return;
        }
        let mut v = ids.get();
        if v.len() >= 60 {
            if !back_to_top_done.get() {
                back_to_top_done.set(true);
                eprintln!("[SMOKE] cap reached — scrolling back to top");
                list_handle.scroll_to_position(0, true);
            }
            return;
        }
        let n = next.get();
        next.set(n + 10);
        v.extend(n..n + 10);
        ids.set(v);
        // Chase via the NEXT layoutcomplete (scrolling now would target
        // an index the NATIVE list doesn't have yet — the append flushes
        // later this tick).
        chase_bottom.set(true);
    };

    let on_upper = |e: whisker::event::ScrollEvent| {
        eprintln!("[SMOKE] scrolltoupper fired: {e:?}");
    };

    // Scenario mutations run once the step-1 scroll SETTLES. Observed
    // iOS states: 4 = animated (programmatic smooth scroll in flight),
    // 1 = idle/settled. Mutating mid-flight would race the animation.
    let on_state = move |e: whisker::event::ScrollStateChangeEvent| {
        eprintln!("[SMOKE] scrollstatechange: state={}", e.detail.state);
        if e.detail.state != 1 || step.get() != 1 {
            return;
        }
        step.set(2);
        match scenario() {
            "prepend" => {
                let n = next.get();
                next.set(n + 3);
                let mut v = ids.get();
                for k in n..n + 3 {
                    v.insert(0, k);
                }
                eprintln!("[SMOKE] prepending 3 rows at top (anchored mid-list)");
                ids.set(v);
            }
            "remove" => {
                let v: Vec<u32> = ids
                    .get()
                    .into_iter()
                    .filter(|n| !(1..=5).contains(n))
                    .collect();
                eprintln!("[SMOKE] removing rows 1..=5 (above the viewport)");
                ids.set(v);
            }
            "upper" => {
                eprintln!("[SMOKE] at bottom — scrolling back toward the top");
                list_handle.scroll_to_position(0, true);
            }
            _ => {}
        }
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
                lower_threshold_item_count: 2,
                upper_threshold_item_count: 2,
                sticky: scen == "sticky",
                list_type: if scen == "waterfall" {
                    whisker::attrs::ListType::Waterfall
                } else {
                    whisker::attrs::ListType::Single
                },
                span_count: if scen == "waterfall" { 2i32 } else { 1i32 },
                initial_scroll_index: if scen == "initial" { 15i32 } else { -1i32 },
                on_scrolltolower: on_lower,
                on_scrolltoupper: on_upper,
                on_scrollstatechange: on_state,
                // Event-pipeline smoke signals: `layoutcomplete` fires on
                // first layout (no interaction needed), `scroll` on every
                // scroll frame. Both are core-originated — they only fire
                // when the capi custom-event channel works end-to-end.
                ref: list_handle.r(),
                on_layoutcomplete: move |e| {
                    eprintln!("[SMOKE] layoutcomplete fired: {e:?}");
                    let seen = lc_seen.get() + 1;
                    lc_seen.set(seen);
                    if seen == data_lc && step.get() == 0 {
                        match scenario() {
                            "" => {
                                // Full default flow: ride to the bottom.
                                list_handle.scroll_to_position(ids.get().len() as i32, true);
                            }
                            "prepend" | "remove" => {
                                step.set(1);
                                list_handle.scroll_to_position(15, true);
                            }
                            "upper" => {
                                step.set(1);
                                list_handle.scroll_to_position(ids.get().len() as i32, true);
                            }
                            "sticky" => {
                                step.set(1);
                                list_handle.scroll_to_position(20, true);
                            }
                            _ => {}
                        }
                    }
                    // Default flow: an append's layout completed — scroll
                    // back to the TOP: the first row was recycled off-screen
                    // during the trip down, so this exercises item-0
                    // re-materialization (regression: it stayed blank).
                    if chase_bottom.get() {
                        chase_bottom.set(false);
                        eprintln!("[SMOKE] append landed — scrolling back to top");
                        list_handle.scroll_to_position(0, true);
                    }
                },
                on_scroll: |e| eprintln!("[SMOKE] scroll fired: {e:?}"),
                each: move || {
                    let mut rows = vec![Row::Header];
                    rows.extend(ids.get().into_iter().map(Row::Item));
                    rows
                },
                key: |r: &Row| r.key(),
                children: |r: Row| match r {
                    Row::Header => render! {
                        list_item(
                            full_span: true,
                            sticky_top: scenario() == "sticky",
                            estimated_size: 160,
                            reuse_identifier: "header",
                        ) {
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
