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

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_image::{Image, ImageEvent, ImageMode, prefetch};

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";
const MUTED: &str = "#9a9aa2";
const OK: &str = "#5bd68a";
const BAD: &str = "#ff5577";

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
    let page = format!(
        "background-color: {BG}; flex-grow: 1; flex-direction: column; \
         padding: 16px; padding-top: 64px;"
    );
    let card = format!(
        "background-color: {CARD_BG}; border-radius: 12px; padding: 12px; \
         margin-bottom: 16px; flex-direction: column;"
    );
    let heading = format!("color: {FG}; font-size: 16px; font-weight: bold;");
    let note = format!("color: {MUTED}; font-size: 13px; margin-top: 4px;");
    let thumb = "width: 100%; height: 140px; border-radius: 8px; margin-top: 8px;";
    let button = format!(
        "background-color: {BAD}; border-radius: 8px; padding: 10px; \
         align-items: center; justify-content: center; margin-top: 8px;"
    );
    let button_text = format!("color: {FG}; font-size: 14px; font-weight: bold;");

    // Card 1 — the header actually leaving the phone.
    // 0 none, 1 an Accept the host likes, 2 one it refuses. Platforms
    // differ in what they send by default — iOS asks for `*/*` and is
    // refused, Android asks for `image/*` and is served — so one step
    // or the other proves the header left the phone on either.
    let accept = RwSignal::new(0u8);
    let probe_status = RwSignal::new("waiting".to_string());
    let probe_style = computed(move || {
        let colour = if probe_status.get().starts_with("loaded") {
            OK
        } else {
            BAD
        };
        format!("color: {colour}; font-size: 13px; margin-top: 4px;")
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
                    style: thumb,
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
                    style: thumb,
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
                    view(style: "flex-direction: row; gap: 8px; margin-top: 8px;") {
                        Image(src: WARM[0], style: "flex-grow: 1; height: 80px;")
                        Image(src: WARM[1], style: "flex-grow: 1; height: 80px;")
                        Image(src: WARM[2], style: "flex-grow: 1; height: 80px;")
                    }
                }
            }
        }
    }
}
