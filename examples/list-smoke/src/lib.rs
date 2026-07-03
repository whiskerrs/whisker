//! Smoke test for whisker's `<list>` (on-demand virtualization + Option E).
//!
//! - A **full-span header** as item 0 (`list_item(full_span, estimated_size)`)
//!   — verifies bugs ② (header crush) / ③ (cross-axis width).
//! - **Variable-height rows** — verifies uniform width + recycling under scroll.
//! - **Rotate / Prepend** buttons mutate the data order — verifies bug ①
//!   (stable item-key reorders correctly instead of appending at the tail).

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

#[whisker::main]
pub fn app() -> Element {
    // Start EMPTY and populate in `on_mount` — the data then arrives
    // AFTER the first layout, exercising the "late data source" fill
    // path (a data-only update must still tick the list's viewport
    // fill; regression: items only materialized on the first scroll).
    let ids = signal(Vec::<u32>::new());
    let next = signal(100u32);
    on_mount(move || ids.set((1u32..=30).collect::<Vec<u32>>()));
    // Scroll-event smoke: on the FIRST layoutcomplete, smooth-scroll to
    // the bottom. A programmatic smooth scroll drives the same native
    // UIScrollView path a finger does, so `scroll` + `scrolltolower`
    // fire without needing a human drag (synthetic touches don't reach
    // Lynx's scroll pipeline on the simulator).
    let list_handle = ListHandle::new();
    let lc_seen = signal(0u32);

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
    // Append a page on scroll-to-bottom — the infinite-scroll pattern.
    // With diff-based data updates the append is insert-only, so the
    // scroll position must HOLD at the bottom (regression: the full
    // replace reset it to the top on every append). At the cap, scroll
    // back to the top once — the first row must re-materialize after
    // having been recycled off-screen.
    let back_to_top_done = signal(false);
    let chase_bottom = signal(false);
    let append_on_scroll = move |e: whisker::event::ScrollEvent| {
        eprintln!("[SMOKE] scrolltolower fired: {e:?}");
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
        // Chase the new bottom ON THE NEXT layoutcomplete (scrolling now
        // would target an index the NATIVE list doesn't have yet — the
        // append flushes later this tick). The chase keeps the smoke
        // paging until the cap: the position HOLD after an append means
        // we drift out of the lower threshold, which is correct UX but
        // would stall the run.
        chase_bottom.set(true);
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
                on_scrolltolower: append_on_scroll,
                // Event-pipeline smoke signals: `layoutcomplete` fires on
                // first layout (no interaction needed), `scroll` on every
                // scroll frame. Both are core-originated — they only fire
                // when the capi custom-event channel works end-to-end.
                ref: list_handle.r(),
                on_layoutcomplete: move |e| {
                    eprintln!("[SMOKE] layoutcomplete fired: {e:?}");
                    // `SIMCTL_CHILD_SMOKE_NO_AUTOSCROLL=1` (simctl launch env)
                    // disables the auto-scroll so the "late data fills the
                    // viewport WITHOUT any scroll" state can be observed.
                    // Trigger on the SECOND layoutcomplete: the first is the
                    // pre-data (header-only) layout — the signal already
                    // holds 30 ids then, but the NATIVE list doesn't yet, so
                    // scrolling on it would target an out-of-range index.
                    let seen = lc_seen.get() + 1;
                    lc_seen.set(seen);
                    if seen == 2 && std::env::var("SMOKE_NO_AUTOSCROLL").is_err() {
                        list_handle.scroll_to_position(ids.get().len() as i32, true);
                    }
                    // An append's layout completed — scroll back to the TOP:
                    // the first row was recycled off-screen during the trip
                    // to the bottom, so this exercises re-materialization of
                    // item 0 (regression: it stayed blank).
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
