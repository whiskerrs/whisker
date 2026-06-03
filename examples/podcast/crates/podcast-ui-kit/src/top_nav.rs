//! Top navigation bar.
//!
//! Three-part bar: leading menu glyph, centred title, trailing
//! action. Visual reference: a generic mobile media-browser header
//! (Pocket Casts, Spotify, Castbox all ship a near-identical
//! shape). The menu glyph is `lucide::Menu` via `whisker-icons`.

use podcast_theme as theme;
use whisker::css::{AlignItems, Display, FlexDirection, FontWeight, JustifyContent, ToCss};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_icons::{lucide, Icon, IconProps};
use whisker_safe_area::safe_area_insets;

/// Title shown centred in the bar. Action label shown trailing.
/// Both are plain strings so the host screen can localise without
/// touching the kit.
#[component]
pub fn top_nav(title: String, action_label: String) -> Element {
    // Two-layer style: outer wrapper carries the safe-area inset
    // via `padding_top`, inner row keeps its content height. We
    // can't pile padding on top of a fixed-height row — the row's
    // children would just disappear behind the padding. Splitting
    // wrapper / row keeps `NAV_HEIGHT` purely about the nav content
    // area.
    //
    // `safe_area_insets()` returns a process-wide `ReadSignal` —
    // re-fires on rotation / Dynamic Island / notch / Android edge-
    // to-edge toggle. `computed` derives a `ReadSignal<String>` the
    // `style:` prop wires as a reactive style binding, so the bar
    // re-pads automatically without the component body touching
    // state.
    let insets = safe_area_insets();
    let wrapper_style = computed(move || {
        css!(
            width: percent(100),
            padding_top: px(insets.get().top as f32),
            flex_shrink: 0.0,
            background_color: theme::BG,
        )
        .to_css_string()
    });

    render! {
        view(style: wrapper_style) {
            view(style: css!(
                width: percent(100),
                min_height: theme::NAV_HEIGHT,
                flex_shrink: 0.0,
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                padding_left: theme::GUTTER,
                padding_right: theme::GUTTER,
            )) {
                // Three flex children — leading / centre / trailing
                // — each claiming an equal third so the title
                // genuinely centres on the bar, not on the title
                // text's own width.
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexStart,
                )) {
                    Icon(
                        svg: lucide::Menu,
                        // `whisker-icons` plumbs `color` straight
                        // through to Lynx as a CSS colour string
                        // (`stroke="currentColor"` in the source
                        // SVG), so the accent hex literal is the
                        // right shape here — there's no typed
                        // `Color` path on `Icon`'s prop yet.
                        color: "#a78bfa",
                        size: "22",
                    )
                }
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_NAV_TITLE,
                            color: theme::TEXT_PRIMARY,
                            font_weight: FontWeight::Numeric(600),
                        ),
                        value: title.clone(),
                    )
                }
                view(style: css!(
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: percent(0),
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                )) {
                    text(
                        style: css!(
                            font_size: theme::T_NAV_TITLE,
                            color: theme::ACCENT,
                            font_weight: FontWeight::Numeric(500),
                        ),
                        value: action_label.clone(),
                    )
                }
            }
        }
    }
}
