//! `whisker-image` example app.
//!
//! Every card here is a question the module can only answer on a device:
//!
//! * **headers** — `https://httpbin.org/image` answers `406` unless the
//!   request carries an `Accept` the host likes, so the same URL loading
//!   or failing is proof of whether the header left the phone.
//! * **on_load** — a public image reports the pixel size it decoded.
//! * **on_error** — a URL that 404s says so, rather than leaving a blank.
//! * **prefetch** — three images warmed before any element points at
//!   them; showing them afterwards should paint from cache.

use whisker::css::{AlignItems, FlexDirection, FontWeight, JustifyContent};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageEvent, ImageMode, prefetch};

const BG: u32 = 0x101012;
const CARD_BG: u32 = 0x1c1c1f;
const FG: u32 = 0xf0f0f3;
const MUTED: u32 = 0x9a9aa2;
const OK: u32 = 0x5bd68a;
const BAD: u32 = 0xff5577;

/// Answers `200 image/png` with an `Accept` it likes and `406` without.
const HEADER_PROBE: &str = "https://httpbin.org/image";
const ACCEPT_PNG: &str = r#"{"Accept": "image/png"}"#;
/// An `Accept` the host refuses, so a platform that sends a usable one
/// by default still shows the header arriving: the load starts failing
/// only because of what we asked for.
const ACCEPT_TEXT: &str = r#"{"Accept": "text/plain"}"#;
const MISSING: &str = "https://httpbin.org/status/404";
const PHOTO: &str = "https://picsum.photos/seed/whisker/600/400";
const WARM: [&str; 3] = [
    "https://picsum.photos/seed/one/300/200",
    "https://picsum.photos/seed/two/300/200",
    "https://picsum.photos/seed/three/300/200",
];

#[whisker::main]
pub fn app() -> Element {
    let page = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .flex_direction(FlexDirection::Column)
        .padding(px(16))
        .padding_top(px(64));
    let card = Css::new()
        .background_color(Color::hex(CARD_BG))
        .border_radius(px(12))
        .padding(px(12))
        .margin_bottom(px(16))
        .flex_direction(FlexDirection::Column);
    let heading = Css::new()
        .color(Color::hex(FG))
        .font_size(px(16))
        .font_weight(FontWeight::Bold);
    let note = Css::new()
        .color(Color::hex(MUTED))
        .font_size(px(13))
        .margin_top(px(4));
    let thumb = Css::new()
        .width(percent(100))
        .height(px(140))
        .border_radius(px(8))
        .margin_top(px(8));
    let button = Css::new()
        .background_color(Color::hex(BAD))
        .border_radius(px(8))
        .padding(px(10))
        .align_items(AlignItems::Center)
        .justify_content(JustifyContent::Center)
        .margin_top(px(8));
    let button_text = Css::new()
        .color(Color::hex(FG))
        .font_size(px(14))
        .font_weight(FontWeight::Bold);

    // Card 1 — the header actually leaving the phone.
    // 0 none, 1 an Accept the host likes, 2 one it refuses. Platforms
    // differ in what they send by default — iOS asks for `*/*` and is
    // refused, Android asks for `image/*` and is served — so one step
    // or the other proves the header left the phone on either.
    let accept = RwSignal::new(0u8);
    let probe_status = RwSignal::new("waiting".to_string());
    let probe_style = computed(move || {
        let color = if probe_status.get().starts_with("loaded") {
            OK
        } else {
            BAD
        };
        Css::new()
            .color(Color::hex(color))
            .font_size(px(13))
            .margin_top(px(4))
    });

    // Card 2 / 3 — the two outcomes reported plainly.
    let photo_status = RwSignal::new("loading…".to_string());
    let missing_status = RwSignal::new("loading…".to_string());

    // Card 4 — warmed before anything points at them.
    let warmed = RwSignal::new(false);

    render! {
        scroll_view(style: page, scroll_orientation: ScrollOrientation::Vertical) {
            view(style: card.clone()) {
                text(value: "headers", style: heading.clone())
                text(
                    value: "httpbin answers 406 without an Accept it likes. \
                            Toggling the header changes nothing else.",
                    style: note.clone(),
                )
                text(value: probe_status, style: probe_style)
                Image(
                    src: HEADER_PROBE,
                    mode: ImageMode::AspectFit,
                    headers: computed(move || match accept.get() {
                        1 => ACCEPT_PNG.to_string(),
                        2 => ACCEPT_TEXT.to_string(),
                        _ => String::new(),
                    }),
                    on_load: move |event: ImageEvent| {
                        probe_status.set(format!(
                            "loaded {}x{}",
                            event.detail.width as i64, event.detail.height as i64
                        ));
                    },
                    on_error: move |event: ImageEvent| {
                        probe_status.set(format!("error: {}", event.error()));
                    },
                    style: thumb.clone(),
                )
                view(
                    style: button.clone(),
                    on_tap: move |_| {
                        probe_status.set("waiting".to_string());
                        accept.set((accept.get_untracked() + 1) % 3);
                    },
                ) {
                    text(
                        value: computed(move || match accept.get() {
                            1 => "Accept: image/png — tap for one the host refuses".to_string(),
                            2 => "Accept: text/plain — tap to drop headers".to_string(),
                            _ => "no headers — tap to send Accept: image/png".to_string(),
                        }),
                        style: button_text.clone(),
                    )
                }
            }

            view(style: card.clone()) {
                text(value: "on_load", style: heading.clone())
                text(value: photo_status, style: note.clone())
                Image(
                    src: PHOTO,
                    mode: ImageMode::AspectFill,
                    on_load: move |event: ImageEvent| {
                        photo_status.set(format!(
                            "{}x{}",
                            event.detail.width as i64, event.detail.height as i64
                        ));
                    },
                    on_error: move |event: ImageEvent| {
                        photo_status.set(format!("error: {}", event.error()));
                    },
                    style: thumb.clone(),
                )
            }

            view(style: card.clone()) {
                text(value: "on_error", style: heading.clone())
                text(value: missing_status, style: note.clone())
                Image(
                    src: MISSING,
                    on_load: move |_: ImageEvent| {
                        missing_status.set("loaded — expected a failure".to_string());
                    },
                    on_error: move |event: ImageEvent| {
                        missing_status.set(format!("error: {}", event.error()));
                    },
                    style: thumb,
                )
            }

            view(style: card) {
                text(value: "prefetch", style: heading)
                text(
                    value: "Warm three URLs, then show them. The second tap \
                            should paint without a network wait.",
                    style: note,
                )
                view(
                    style: button,
                    on_tap: move |_| {
                        prefetch(
                            &WARM.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                            "",
                        );
                        warmed.set(true);
                    },
                ) {
                    text(
                        value: computed(move || {
                            if warmed.get() {
                                "warmed — tap again to show".to_string()
                            } else {
                                "prefetch three images".to_string()
                            }
                        }),
                        style: button_text,
                    )
                }
                Show(when: move || warmed.get()) {
                    view(style: Css::new().flex_direction(FlexDirection::Row).gap(px(8)).margin_top(px(8))) {
                        Image(src: WARM[0], style: css!(flex_grow: 1.0, height: px(80)))
                        Image(src: WARM[1], style: css!(flex_grow: 1.0, height: px(80)))
                        Image(src: WARM[2], style: css!(flex_grow: 1.0, height: px(80)))
                    }
                }
            }
        }
    }
}
