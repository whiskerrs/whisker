//! `whisker-icons` example app.
//!
//! Renders every Lucide icon (~1700) through Whisker's
//! render-props `list(...)` so Lynx's native virtualised list
//! drives the scrolling — only the tiles inside the viewport
//! are materialised on mount.
//!
//! The (label, svg) table lives in `icons.rs` as a plain
//! committed Rust source file. Each row references a `pub const`
//! from `whisker_icons::lucide` directly, so the symbol-level
//! tree-shaking story for downstream consumers is unchanged —
//! this example pulls every icon in because it explicitly
//! enumerates them, but other consumers that touch only a
//! handful still get the trimmed-binary behaviour.
//!
//! Styling is via the typed `css!` macro inline at each call
//! site — no intermediate `format!`-ed `String` styles — so the
//! readability story matches the rest of the kit (`pane.rs`,
//! `tabs.rs`).

mod icons;

use whisker::css::{FontWeight, TextAlign};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::Icon;
use whisker_safe_area::safe_area_insets;

#[whisker::main]
pub fn app() -> Element {
    let header_text = format!("lucide gallery (all {})", icons::ALL.len());
    // Reactive page padding — picks up the status-bar / notch
    // height on iOS and (post-WhiskerActivity-edge-to-edge) Android.
    // Re-fires on rotation / Dynamic Island / system-bar visibility
    // changes via `safe_area_insets()`'s process-wide signal.
    let insets = safe_area_insets();

    render! {
        view(style: computed(move || css!(
            background_color: Color::hex(0x101012),
            flex_grow: 1.0,
            flex_shrink: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            padding_top: px(insets.get().top as f32 + 16.0),
            padding_bottom: px(insets.get().bottom as f32 + 16.0),
        ))) {
            // Wrap the header `text` in a `view` — a known Lynx
            // Android quirk collapses the first direct child of the
            // root `<page>` to zero height when edge-to-edge is enabled
            // (`WhiskerActivity` flips `setDecorFitsSystemWindows(false)`).
            // Putting an intermediate flex container in between gives
            // the text a frame to lay out against. The wrapper is
            // otherwise transparent.
            view(style: css!(
                width: percent(100),
                flex_shrink: 0.0,
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xF0F0F3),
                        font_size: px(22),
                        font_weight: FontWeight::Numeric(700),
                        margin_left: px(20),
                        margin_bottom: px(16),
                    ),
                    value: header_text,
                )
            }
            list(
                each: || icons::ALL.to_vec(),
                meta: |(name, _): &(&'static str, &'static str)| {
                    ItemMeta::key(name.to_string())
                },
                children: |(name, svg): (&'static str, &'static str)| render! {
                    tile(label: name, svg: svg)
                },
                // `list-type: "flow"` activates Lynx's multi-column
                // flow layout. iOS Lynx reads `span-count`; Android
                // Lynx reads `column-count` — set both so the same
                // Rust source grids on each platform.
                list_type: ListType::Flow,
                column_count: 3,
                span_count: 3,
                // Lynx's `<list>` needs a bounded height to virtualise —
                // `flex_grow: 1` inside the flex-column page gives it
                // whatever's left under the header.
                style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    width: percent(100),
                ),
            )
        }
    }
}

/// One labelled tile. Reused for every icon — the only varying
/// inputs are the label and the SVG body. Color and size are
/// fixed; per-icon tinting can be added later without touching
/// the list plumbing.
#[component]
fn tile(label: &'static str, svg: &'static str) -> Element {
    render! {
        // `width: 100%` makes the card fill the list-cell so
        // `align-items: center` (cross-axis = horizontal under
        // `flex-direction: column`) actually has a frame to center
        // against — without it the card collapses to its intrinsic
        // content width and pins to the left edge of the cell.
        view(style: css!(
            width: percent(100),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: px(8),
        )) {
            view(style: css!(
                width: px(64),
                height: px(64),
                background_color: Color::hex(0x1C1C1F),
                border_radius: px(12),
                display: Display::Flex,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                Icon(svg: svg, color: "#f0f0f3", size: "32")
            }
            text(
                style: css!(
                    color: Color::hex(0xF0F0F3),
                    font_size: px(10),
                    margin_top: px(4),
                    text_align: TextAlign::Center,
                ),
                value: label,
            )
        }
    }
}
