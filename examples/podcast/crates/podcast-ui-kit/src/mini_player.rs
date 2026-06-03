//! Bottom mini-player bar.
//!
//! Floats above the scrolling content. Three elements: a leading
//! placeholder artwork square (sized for the future "currently
//! playing show art"), a play glyph, and a "skip ahead 30 s" glyph
//! on the trailing edge. No audio wiring yet — the buttons are
//! visual-only until the `whisker-audio` module lands.

use podcast_theme as theme;
use whisker::prelude::*;
use whisker::runtime::view::Element;

#[component]
pub fn mini_player() -> Element {
    // Floats above content via absolute positioning. The parent
    // page must use `position: relative` (default) so the floats
    // anchor correctly. Side gutter matches the page gutter so the
    // bar visually inset-aligns with the section content.
    let bar_style = format!(
        "position: absolute; \
         left: {gutter}; right: {gutter}; \
         bottom: {bottom}; height: {h}; \
         display: flex; flex-direction: row; align-items: center; \
         padding-left: 12px; padding-right: 16px; \
         border-radius: 12px; \
         background-color: {bg};",
        gutter = theme::GUTTER,
        bottom = theme::MINI_PLAYER_BOTTOM,
        h = theme::MINI_PLAYER_HEIGHT,
        bg = theme::MINI_PLAYER_BG,
    );

    // Leading placeholder: 40×40 square where the now-playing show
    // art would go.
    let art_style = "width: 36px; height: 36px; \
                     border-radius: 6px; \
                     background-color: rgba(255, 255, 255, 0.15);"
        .to_string();

    // Spacer between leading art and trailing controls.
    let spacer_style = "flex-grow: 1; flex-shrink: 1;".to_string();

    // Play glyph — a right-pointing triangle made from a rotated
    // bordered view. (We could text-render '▶' instead; views are
    // sturdier when font fallback is uncertain.)
    let play_wrap_style = "width: 32px; height: 32px; \
                           display: flex; flex-direction: row; \
                           align-items: center; justify-content: center;"
        .to_string();
    let play_glyph_style = format!("font-size: 18px; color: {fg};", fg = theme::TEXT_PRIMARY,);

    // Skip-30 glyph — a circular outline with "30" inside, rendered
    // text-only.
    let skip_wrap_style = "width: 36px; height: 36px; \
                           display: flex; flex-direction: row; \
                           align-items: center; justify-content: center; \
                           border-radius: 18px; \
                           border-width: 1.5px; border-style: solid; \
                           border-color: rgba(255,255,255,0.85); \
                           margin-left: 16px;"
        .to_string();
    let skip_label_style = format!(
        "font-size: 11px; color: {fg}; font-weight: 600;",
        fg = theme::TEXT_PRIMARY,
    );

    render! {
        view(style: bar_style) {
            view(style: art_style)
            view(style: spacer_style)
            view(style: play_wrap_style) {
                text(style: play_glyph_style, value: "▶")
            }
            view(style: skip_wrap_style) {
                text(style: skip_label_style, value: "30")
            }
        }
    }
}
