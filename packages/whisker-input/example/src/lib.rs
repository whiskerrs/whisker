//! `whisker-input` example app.
//!
//! Exercises the four headline usage modes end-to-end on a real
//! device so a `whisker run` round-trip verifies the native module
//! wiring:
//!
//! * **Two-way** — an [`Input`] bound to an `RwSignal<String>`, with a
//!   live `<text>` preview that updates on every keystroke.
//! * **Controlled** — an [`Input`] driven by a `value:` signal whose
//!   writeback upper-cases each keystroke (escape-hatch shape).
//! * **Multiline** — a `lines: 4` notes area.
//! * **Secure** — a masked password field.

use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_input::{Input, KeyboardType};

const BG: u32 = 0x101012;
const CARD_BG: u32 = 0x1c1c1f;
const FG: u32 = 0xf0f0f3;
const MUTED: &str = "#9a9aa2";
const ACCENT: &str = "#ff5577";

#[whisker::main]
pub fn app() -> Element {
    let page_style = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .flex_shrink(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(56))
        .padding_left(px(20))
        .padding_right(px(20));
    let header_style = Css::new()
        .color(Color::hex(FG))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(20));

    render! {
        View(style: page_style) {
            Text(style: header_style, value: "whisker-input demo")

            TwoWayDemo()
            ControlledDemo()
            MultilineDemo()
            SecureDemo()
        }
    }
}

/// Two-way bound field + a live preview of the bound signal.
#[component]
fn two_way_demo() -> Element {
    let text = RwSignal::new(String::new());
    let preview = Css::new()
        .color(Color::hex(0x9a9aa2))
        .font_size(px(14))
        .margin_top(px(6));

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "Two-way binding")
            Input(
                text: text,
                placeholder: "Type something…",
                placeholder_color: MUTED,
                caret_color: ACCENT,
                style: field_style(),
            )
            Text(
                style: preview,
                value: computed(move || format!("Bound value: {}", text.get())),
            )
        }
    }
}

/// Controlled field — `value:` is the source of truth and the
/// writeback upper-cases each keystroke.
#[component]
fn controlled_demo() -> Element {
    let value = signal(String::new());

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "Controlled (UPPER-CASE)")
            Input(
                value: value,
                on_input: move |s: String| value.set(s.to_uppercase()),
                placeholder: "lowercase becomes UPPER",
                placeholder_color: MUTED,
                keyboard_type: KeyboardType::Email,
                style: field_style(),
            )
        }
    }
}

/// Multiline notes area, fixed at 4 visible lines.
#[component]
fn multiline_demo() -> Element {
    let notes = RwSignal::new(String::new());
    let area_style = Css::new()
        .background_color(Color::hex(CARD_BG))
        .color(Color::hex(FG))
        .font_size(px(16))
        .border_radius(px(10))
        .padding(px(12))
        .min_height(px(96));

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "Multiline (4 lines)")
            Input(
                text: notes,
                multiline: true,
                lines: 4u32,
                placeholder: "Notes…",
                placeholder_color: MUTED,
                style: area_style,
            )
        }
    }
}

/// Secure (masked) password field.
#[component]
fn secure_demo() -> Element {
    let password = RwSignal::new(String::new());

    render! {
        View(style: section_style()) {
            Text(style: label_style(), value: "Secure (password)")
            Input(
                text: password,
                secure: true,
                placeholder: "Password",
                placeholder_color: MUTED,
                return_key: whisker_input::ReturnKey::Done,
                style: field_style(),
            )
        }
    }
}

// ---- Shared styling --------------------------------------------------------

fn section_style() -> Css {
    Css::new()
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .margin_bottom(px(24))
}

fn label_style() -> Css {
    Css::new()
        .color(Color::hex(FG))
        .font_size(px(13))
        .font_weight(FontWeight::Numeric(600))
        .margin_bottom(px(8))
}

fn field_style() -> Css {
    Css::new()
        .background_color(Color::hex(CARD_BG))
        .color(Color::hex(FG))
        .font_size(px(16))
        .height(px(48))
        .border_radius(px(10))
        .padding_left(px(12))
        .padding_right(px(12))
}
