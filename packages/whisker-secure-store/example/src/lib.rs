//! `whisker-secure-store` example app.
//!
//! On launch it runs a `save → load → remove → load` round-trip against
//! the platform secure store (iOS Keychain / Android Tink + Keystore)
//! and renders each step's result, so a `whisker run` on a real device
//! verifies the native module wiring end-to-end.

use whisker::css::{FlexDirection, FontWeight};
use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_secure_store::WhiskerSecureStore;

const BG: u32 = 0x101012;
const FG: u32 = 0xf0f0f3;

#[whisker::main]
pub fn app() -> Element {
    let log = RwSignal::new("running secure-store round-trip…".to_string());

    on_mount(move || {
        let key = "demo.session".to_string();
        let secret = "tok_abc.dpop_xyz".to_string();
        let mut out = String::new();

        match WhiskerSecureStore::save(key.clone(), secret.clone()) {
            Ok(true) => out.push_str("save: ok\n"),
            Ok(false) => out.push_str("save: returned false\n"),
            Err(e) => out.push_str(&format!("save: ERROR {e}\n")),
        }
        match WhiskerSecureStore::load(key.clone()) {
            Ok(Some(v)) if v == secret => out.push_str("load: matches saved value\n"),
            Ok(Some(v)) => out.push_str(&format!("load: MISMATCH {v}\n")),
            Ok(None) => out.push_str("load: None (expected a value)\n"),
            Err(e) => out.push_str(&format!("load: ERROR {e}\n")),
        }
        match WhiskerSecureStore::remove(key.clone()) {
            Ok(()) => out.push_str("remove: ok\n"),
            Err(e) => out.push_str(&format!("remove: ERROR {e}\n")),
        }
        match WhiskerSecureStore::load(key.clone()) {
            Ok(None) => out.push_str("load after remove: None (correct)\n"),
            Ok(Some(v)) => out.push_str(&format!("load after remove: STILL PRESENT {v}\n")),
            Err(e) => out.push_str(&format!("load after remove: ERROR {e}\n")),
        }
        log.set(out);
    });

    let page = Css::new()
        .background_color(Color::hex(BG))
        .flex_grow(1.0)
        .display_flex()
        .flex_direction(FlexDirection::Column)
        .padding_top(px(72))
        .padding_left(px(20))
        .padding_right(px(20));
    let title = Css::new()
        .color(Color::hex(FG))
        .font_size(px(22))
        .font_weight(FontWeight::Numeric(700))
        .margin_bottom(px(20));
    let body = Css::new()
        .color(Color::hex(FG))
        .font_size(px(16))
        .line_height(px(28));

    render! {
        View(style: page) {
            Text(style: title, value: "whisker-secure-store")
            Text(style: body, value: computed(move || log.get()))
        }
    }
}
