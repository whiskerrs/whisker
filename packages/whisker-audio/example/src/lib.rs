//! `whisker-audio` example app.
//!
//! Instantiates one [`Player`] against a CDN-hosted MP3, displays
//! the live playback position bound to a reactive signal, and
//! renders Play / Pause / Stop / Seek tap targets.

use whisker::css::{AlignItems, Color, Display, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_audio::Player;
use whisker_safe_area::safe_area_insets;

/// Stable public-domain MP3 hosted on a Google CDN. Loads
/// reliably from emulators, no auth, ~3 MB.
const SAMPLE_URL: &str = "https://commondatastorage.googleapis.com/codeskulptor-demos/\
    DDR_assets/Kangaroo_MusiQue_-_The_Neverwritten_Role_Playing_Game.mp3";

#[whisker::main]
pub fn app() -> Element {
    // The player owns its native ExoPlayer/AVPlayer until every
    // clone of this handle drops. Held by the surrounding owner
    // (the `#[whisker::main]` body) so it lives for the app's
    // lifetime.
    let player = Player::new(SAMPLE_URL);
    let status = player.status();
    let insets = safe_area_insets();

    render! {
        page(style: computed(move || css!(
            background_color: Color::hex(0x101012),
            flex_grow: 1.0,
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            padding_top: px(insets.get().top as f32 + 24.0),
            padding_bottom: px(insets.get().bottom as f32 + 24.0),
            padding_left: px(24),
            padding_right: px(24),
        ))) {
            // Header
            view(style: css!(
                width: percent(100),
                flex_shrink: 0.0,
                margin_bottom: px(24),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xF0F0F3),
                        font_size: px(22),
                        font_weight: FontWeight::Numeric(700),
                    ),
                    value: "whisker-audio demo",
                )
            }

            // Live `PlaybackStatus`. Each row re-renders independently
            // — `status.get()` inside the `computed` closure registers
            // it as a dependent of the underlying RwSignal that the
            // native module writes through.
            view(style: css!(
                width: percent(100),
                max_width: px(320),
                flex_shrink: 0.0,
                margin_bottom: px(24),
                padding: px(16),
                background_color: Color::hex(0x18181B),
                border_radius: px(12),
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
            )) {
                status_row(label: "is_playing", value: computed(move || {
                    fmt_bool(status.get().is_playing)
                }))
                status_row(label: "is_loaded", value: computed(move || {
                    fmt_bool(status.get().is_loaded)
                }))
                status_row(label: "position", value: computed(move || {
                    format!("{:.2}s", status.get().position)
                }))
                status_row(label: "duration", value: computed(move || {
                    let d = status.get().duration;
                    if d > 0.0 { format!("{:.2}s", d) } else { "—".into() }
                }))
                // Progress fraction — only meaningful once `duration`
                // is non-zero. The `format!` is cheap; the surrounding
                // computed re-runs on every status tick.
                status_row(label: "progress", value: computed(move || {
                    let s = status.get();
                    if s.duration > 0.0 {
                        format!("{:.1}%", 100.0 * s.position / s.duration)
                    } else {
                        "—".into()
                    }
                }))
            }

            // Controls. Each closure captures its own clone of the
            // handle. Inline `style:` to keep the example focused.
            view(style: button_style(), on_tap: {
                let p = player.clone();
                move |_| p.play()
            }) { text(style: button_text_style(), value: "Play") }
            view(style: button_style(), on_tap: {
                let p = player.clone();
                move |_| p.pause()
            }) { text(style: button_text_style(), value: "Pause") }
            view(style: button_style(), on_tap: {
                let p = player.clone();
                move |_| p.stop()
            }) { text(style: button_text_style(), value: "Stop") }
            view(style: button_style(), on_tap: {
                let p = player.clone();
                move |_| p.seek_to(30.0)
            }) { text(style: button_text_style(), value: "Seek to 30s") }
        }
    }
}

fn button_style() -> Css {
    css!(
        width: percent(100),
        max_width: px(280),
        height: px(48),
        background_color: Color::hex(0x1C1C1F),
        border_radius: px(12),
        margin_top: px(8),
        margin_bottom: px(8),
        display: Display::Flex,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
    )
}

fn button_text_style() -> Css {
    css!(
        color: Color::hex(0xF0F0F3),
        font_size: px(15),
        font_weight: FontWeight::Numeric(600),
    )
}

fn fmt_bool(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

/// One labelled row in the status card. Two `<text>` siblings — the
/// label pinned to the left, the live value to the right, separated
/// by a `flex_grow: 1` filler so the value column right-aligns.
#[component]
fn status_row(label: &'static str, value: ReadSignal<String>) -> Element {
    render! {
        view(style: css!(
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            margin_top: px(4),
            margin_bottom: px(4),
        )) {
            text(
                style: css!(
                    color: Color::hex(0x9090A0),
                    font_size: px(13),
                    font_weight: FontWeight::Numeric(500),
                    width: px(96),
                    flex_shrink: 0.0,
                ),
                value: label,
            )
            text(
                style: css!(
                    color: Color::hex(0xF0F0F3),
                    font_size: px(13),
                    font_weight: FontWeight::Numeric(500),
                    flex_grow: 1.0,
                ),
                value: value,
            )
        }
    }
}
