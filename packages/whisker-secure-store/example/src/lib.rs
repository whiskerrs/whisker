//! `whisker-secure-store` example app.
//!
//! On launch it runs a `save → load → remove → load` round-trip against
//! the platform secure store (iOS Keychain / Android Tink + Keystore)
//! and renders each step's result, so a `whisker run` on a real device
//! verifies the native module wiring end-to-end.

use whisker::prelude::*;
use whisker::runtime::view::Element;
use whisker_secure_store::WhiskerSecureStore;

const BG: &str = "#101012";
const FG: &str = "#f0f0f3";

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

    let page = format!(
        "background-color: {BG}; flex-grow: 1; display: flex; flex-direction: column; \
         padding-top: 72px; padding-left: 20px; padding-right: 20px;"
    );
    let title = format!("color: {FG}; font-size: 22px; font-weight: 700; margin-bottom: 20px;");
    let body = format!("color: {FG}; font-size: 16px; line-height: 28px;");

    render! {
        view(style: page) {
            text(style: title, value: "whisker-secure-store")
            text(style: body, value: computed(move || log.get()))
        }
    }
}
