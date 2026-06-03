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

mod icons;

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{Icon, IconProps};

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";

#[whisker::main]
pub fn app() -> Element {
    let page_style = format!(
        "background-color: {BG}; flex-grow: 1; flex-shrink: 1; \
         display: flex; flex-direction: column; \
         padding-top: 48px; padding-bottom: 24px;",
    );
    let header_style = format!(
        "color: {FG}; font-size: 22px; font-weight: 700; \
         margin-left: 20px; margin-bottom: 16px;",
    );
    let header_text = format!("lucide gallery (all {})", icons::ALL.len());
    // Lynx's `<list>` needs a bounded height to virtualise — `flex-grow:1`
    // inside the flex-column page gives it whatever's left under the header.
    let list_style = "flex-grow: 1; flex-shrink: 1; width: 100%;".to_string();

    render! {
        page(style: page_style) {
            text(style: header_style, value: header_text)
            list(
                each: || icons::ALL.to_vec(),
                key: |(name, _): &(&'static str, &'static str)| name.to_string(),
                children: |(name, svg): (&'static str, &'static str)| render! {
                    tile(label: name, svg: svg)
                },
                // `list-type: "flow"` activates Lynx's multi-column
                // flow layout. iOS Lynx reads `span-count`; Android
                // Lynx reads `column-count` — set both so the same
                // Rust source grids on each platform.
                list_type: "flow",
                column_count: 3,
                span_count: 3,
                style: list_style,
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
    // `width: 100%` makes the card fill the list-cell so
    // `align-items: center` (cross-axis = horizontal under
    // `flex-direction: column`) actually has a frame to center
    // against — without it the card collapses to its intrinsic
    // content width and pins to the left edge of the cell.
    let card_style = "width: 100%; \
                      display: flex; flex-direction: column; align-items: center; \
                      padding: 8px;"
        .to_string();
    let frame_style = format!(
        "width: 64px; height: 64px; \
         background-color: {CARD_BG}; \
         border-radius: 12px; \
         display: flex; align-items: center; justify-content: center;",
    );
    let caption_style = format!(
        "color: {FG}; font-size: 10px; margin-top: 4px; \
         text-align: center;",
    );

    render! {
        view(style: card_style) {
            view(style: frame_style) {
                Icon(svg: svg, color: FG, size: "32")
            }
            text(style: caption_style, value: label)
        }
    }
}
