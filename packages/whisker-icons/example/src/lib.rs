//! `whisker-icons` example app.
//!
//! Renders a 4x4 Lucide gallery so the tree-shaken constant
//! pipeline can be poked end-to-end on a real device. The
//! icons here are a deliberately small slice (16 of the ~1700
//! available) — anything not referenced gets DCE'd out of the
//! resulting binary, which is the whole point of the const-per-icon
//! shape in `whisker_icons::lucide`.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{lucide, Icon, IconProps};

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";
const ACCENT: &str = "#ff5577";

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
    let grid_style = "display: flex; flex-direction: row; flex-wrap: wrap; \
                      padding-left: 12px; padding-right: 12px;"
        .to_string();

    render! {
        page(style: page_style) {
            text(style: header_style, value: "lucide gallery")
            view(style: grid_style) {
                // Row 1 — navigation
                tile(label: "ChevronLeft",  svg: lucide::ChevronLeft,  color: FG)
                tile(label: "ChevronRight", svg: lucide::ChevronRight, color: FG)
                tile(label: "ChevronUp",    svg: lucide::ChevronUp,    color: FG)
                tile(label: "ChevronDown",  svg: lucide::ChevronDown,  color: FG)
                // Row 2 — actions
                tile(label: "Search",       svg: lucide::Search,       color: FG)
                tile(label: "Settings",     svg: lucide::Settings,     color: FG)
                tile(label: "Check",        svg: lucide::Check,        color: FG)
                tile(label: "X",            svg: lucide::X,            color: FG)
                // Row 3 — content
                tile(label: "Heart",        svg: lucide::Heart,        color: ACCENT)
                tile(label: "Star",         svg: lucide::Star,         color: ACCENT)
                tile(label: "Bell",         svg: lucide::Bell,         color: FG)
                tile(label: "Bookmark",     svg: lucide::Bookmark,     color: FG)
                // Row 4 — system
                tile(label: "User",         svg: lucide::User,         color: FG)
                tile(label: "Mail",         svg: lucide::Mail,         color: FG)
                tile(label: "Clock",        svg: lucide::Clock,        color: FG)
                tile(label: "Trash",        svg: lucide::Trash,        color: FG)
            }
        }
    }
}

/// One labelled gallery tile. Same shape for every entry so the
/// visual delta between tiles is the icon alone.
#[component]
fn tile(label: String, svg: String, color: String) -> Element {
    let card_style = "width: 25%; \
                      display: flex; flex-direction: column; align-items: center; \
                      padding: 12px;"
        .to_string();
    let frame_style = format!(
        "width: 64px; height: 64px; \
         background-color: {CARD_BG}; \
         border-radius: 12px; \
         display: flex; align-items: center; justify-content: center;",
    );
    let caption_style = format!("color: {FG}; font-size: 11px; margin-top: 6px;");

    render! {
        view(style: card_style) {
            view(style: frame_style) {
                Icon(svg: svg.clone(), color: color.clone(), size: "32")
            }
            text(style: caption_style, value: label.clone())
        }
    }
}
