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

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_input::{Input, KeyboardType};

const BG: &str = "#101012";
const CARD_BG: &str = "#1c1c1f";
const FG: &str = "#f0f0f3";
const MUTED: &str = "#9a9aa2";
const ACCENT: &str = "#ff5577";

#[whisker::main]
pub fn app() -> Element {
    let page_style = format!(
        "background-color: {BG}; flex-grow: 1; flex-shrink: 1; \
         display: flex; flex-direction: column; \
         padding-top: 56px; padding-left: 20px; padding-right: 20px;",
    );
    let header_style =
        format!("color: {FG}; font-size: 22px; font-weight: 700; margin-bottom: 20px;",);

    render! {
        view(style: page_style) {
            text(style: header_style, value: "whisker-input demo")

            two_way_demo()
            controlled_demo()
            multiline_demo()
            secure_demo()
        }
    }
}

/// Two-way bound field + a live preview of the bound signal.
#[component]
fn two_way_demo() -> Element {
    let text = RwSignal::new(String::new());
    let preview = format!("color: {MUTED}; font-size: 14px; margin-top: 6px;");

    render! {
        view(style: section_style()) {
            text(style: label_style(), value: "Two-way binding")
            Input(
                text: text,
                placeholder: "Type something…",
                placeholder_color: MUTED,
                caret_color: ACCENT,
                style: field_style(),
            )
            text(
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
        view(style: section_style()) {
            text(style: label_style(), value: "Controlled (UPPER-CASE)")
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
    let area_style = format!(
        "background-color: {CARD_BG}; color: {FG}; \
         font-size: 16px; border-radius: 10px; \
         padding: 12px; min-height: 96px;",
    );

    render! {
        view(style: section_style()) {
            text(style: label_style(), value: "Multiline (4 lines)")
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
        view(style: section_style()) {
            text(style: label_style(), value: "Secure (password)")
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

fn section_style() -> String {
    "display: flex; flex-direction: column; margin-bottom: 24px;".to_string()
}

fn label_style() -> String {
    format!("color: {FG}; font-size: 13px; font-weight: 600; margin-bottom: 8px;")
}

fn field_style() -> String {
    format!(
        "background-color: {CARD_BG}; color: {FG}; \
         font-size: 16px; height: 48px; border-radius: 10px; \
         padding-left: 12px; padding-right: 12px;",
    )
}
