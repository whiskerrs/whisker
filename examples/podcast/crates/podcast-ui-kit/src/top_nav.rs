//! Top navigation bar.
//!
//! Three-part bar: leading menu glyph, centred title, trailing
//! action. Visual reference: a generic mobile media-browser header
//! (Pocket Casts, Spotify, Castbox all ship a near-identical
//! shape). Renders text-only — no SVG / image assets are baked in,
//! keeping the component self-contained.

use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_safe_area::safe_area_insets;

/// Title shown centred in the bar. Action label shown trailing.
/// Both are plain strings so the host screen can localise without
/// touching the kit.
#[component]
pub fn top_nav(title: String, action_label: String) -> Element {
    // Two-layer style: outer wrapper carries the safe-area inset
    // via `padding-top`, inner row keeps its content height. We
    // can't pile padding on top of a fixed-height row — the row's
    // children would just disappear behind the padding. Splitting
    // wrapper / row keeps `NAV_HEIGHT` purely about the nav
    // content area.
    //
    // `safe_area_insets()` returns a process-wide `ReadSignal` —
    // re-fires on rotation / Dynamic Island / notch / Android edge-
    // to-edge toggle. `computed` derives a `ReadSignal<String>` the
    // `style:` prop wires as a reactive style binding, so the bar
    // re-pads automatically without us touching state from the
    // component body.
    let insets = safe_area_insets();
    let bg = theme::BG;
    let wrapper_style = computed(move || {
        format!(
            "width: 100%; padding-top: {top}px; \
             flex-shrink: 0; \
             background-color: {bg};",
            top = insets.get().top,
        )
    });
    let bar_style = format!(
        "width: 100%; min-height: {h}; \
         flex-shrink: 0; \
         display: flex; flex-direction: row; align-items: center; \
         padding-left: {gutter}; padding-right: {gutter};",
        h = theme::NAV_HEIGHT,
        gutter = theme::GUTTER,
    );
    // Three flex children — leading / centre / trailing — each
    // claiming an equal third so the title genuinely centres on
    // the bar, not on the title text's own width. The leading
    // slot is a stack of three short bars (a hamburger glyph
    // rendered with views, not an icon font, so the kit doesn't
    // need to ship glyph assets).
    let slot_third = "flex-grow: 1; flex-shrink: 1; flex-basis: 0%; \
                      display: flex; flex-direction: row;";
    let leading_style = format!("{slot_third} align-items: center; justify-content: flex-start;");
    let centre_style = format!("{slot_third} align-items: center; justify-content: center;");
    let trailing_style = format!("{slot_third} align-items: center; justify-content: flex-end;");

    let title_style = format!(
        "font-size: {size}; color: {fg}; font-weight: 600;",
        size = theme::T_NAV_TITLE,
        fg = theme::TEXT_PRIMARY,
    );
    let action_style = format!(
        "font-size: {size}; color: {accent}; font-weight: 500;",
        size = theme::T_NAV_TITLE,
        accent = theme::ACCENT,
    );

    // Hamburger glyph: three stacked 2px-tall purple lines.
    let hamburger_wrap_style = "display: flex; flex-direction: column; width: 18px;".to_string();
    let bar_line = format!(
        "width: 18px; height: 2px; background-color: {accent}; \
         border-radius: 1px; margin-top: 3px; margin-bottom: 3px;",
        accent = theme::ACCENT,
    );

    render! {
        view(style: wrapper_style) {
            view(style: bar_style) {
                view(style: leading_style) {
                    view(style: hamburger_wrap_style) {
                        view(style: bar_line.clone())
                        view(style: bar_line.clone())
                        view(style: bar_line.clone())
                    }
                }
                view(style: centre_style) {
                    text(style: title_style, value: title.clone())
                }
                view(style: trailing_style) {
                    text(style: action_style, value: action_label.clone())
                }
            }
        }
    }
}
