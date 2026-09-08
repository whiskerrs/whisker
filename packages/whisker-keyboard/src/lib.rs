//! `whisker-keyboard` — the on-screen keyboard as a reactive resource.
//!
//! Two capabilities, both routed through one native `Keyboard` module:
//!
//! - [`keyboard_height`] — a runtime-local
//!   `ReadSignal<f64>` carrying the keyboard's current overlap from the
//!   bottom of the Screen (points on iOS, dp on Android), `0.0` when
//!   hidden. Pad or scroll a container by this value so a focused input
//!   isn't covered. This is Whisker's analogue of Flutter's
//!   `MediaQuery.viewInsets.bottom` / React Native's
//!   `keyboardWillShow` end-coordinates.
//! - [`dismiss`] — a **real global unfocus**, not merely a
//!   "hide the keyboard". iOS resigns the key window's first responder
//!   (`endEditing(true)`); Android clears focus on the focused view
//!   (`clearFocus()`) *and* hides the IME. Removing focus — rather than
//!   only hiding the soft keyboard — is what prevents a **hardware
//!   keyboard** from continuing to type into an input that has scrolled
//!   or navigated off Screen (on Android, `hideSoftInputFromWindow`
//!   alone leaves the field focused). Mirrors React Native's
//!   `Keyboard.dismiss()` / Flutter's `FocusManager.primaryFocus.unfocus()`.
//!
//! Web dismisses the active DOM element and reports a fixed height of `0.0`;
//! browser viewport resizing remains the layout engine's responsibility.
//! Desktop currently provides a successful no-op dismiss and the same fixed
//! zero height without linking a Host adapter.
//!
//! ## Usage
//!
//! Keyboard avoidance — pad a scroll container by the keyboard height:
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_keyboard::keyboard_height;
//!
//! #[component]
//! fn form() -> Element {
//!     let kb = keyboard_height();
//!     let pad = move || format!("padding-bottom: {}px;", kb.get());
//!     render! {
//!         ScrollView(style: pad()) { /* fields … */ }
//!     }
//! }
//! ```
//!
//! Dismiss on demand (e.g. a "Done" button or tap-outside):
//!
//! ```ignore
//! whisker_keyboard::dismiss();
//! ```
//!
//! `whisker-router` also calls the native `dismiss` at every navigation
//! so the keyboard goes down (and focus is genuinely released) when the
//! user moves between screens — see that crate. That wiring only fires
//! when this module is present in the app; add `whisker-keyboard` as a
//! dependency to get it.
//!
//! ## Native source
//!
//! - iOS: `packages/whisker-keyboard/ios/Sources/WhiskerKeyboard/KeyboardModule.swift`
//! - Android: `packages/whisker-keyboard/android/src/main/kotlin/rs/whisker/modules/keyboard/KeyboardModule.kt`

#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32", test))]
use whisker::WhiskerValue;
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
use whisker::module;
use whisker::runtime::module::ModuleSubscription;
use whisker::{ReadSignal, RwSignal};

/// Dismiss the keyboard by releasing focus globally.
///
/// This is a **real unfocus**: iOS `endEditing(true)` on the key
/// window (resign first responder), Android `clearFocus()` on the
/// focused view + IME hide. A no-op when nothing is focused. Safe to
/// call from any Whisker event handler; the native side marshals the
/// UIKit / Android View work to the main thread.
pub fn dismiss() {
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        let _ = module!("Keyboard").invoke("dismiss", vec![]);
    }
}

/// Reactive accessor for the on-screen keyboard's current height —
/// the overlap from the bottom of the screen in points (iOS) / dp
/// (Android), `0.0` when the keyboard is hidden.
///
/// Calls share a signal within the entered Runtime; its shutdown releases the subscription.
/// The returned handle must only be used in that Runtime on its UI thread.
pub fn keyboard_height() -> ReadSignal<f64> {
    HEIGHT.with(|slot| slot.read)
}

struct Slot {
    read: ReadSignal<f64>,
    _subscription: Option<ModuleSubscription>,
}

whisker::runtime_local! {
    static HEIGHT: Slot = {
        let signal = RwSignal::new(0.0_f64);
        Slot { read: signal.read_only(), _subscription: subscribe_to_native(signal) }
    };
}

fn subscribe_to_native(writer: RwSignal<f64>) -> Option<ModuleSubscription> {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        let _ = writer;
        None
    }
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        let sub = module!("Keyboard").on_event("keyboardChanged", move |payload| {
            if let Some(height) = decode_payload(payload) {
                writer.set(height);
            }
        });
        if let Some(err) = sub.error() {
            eprintln!("[whisker-keyboard] failed to subscribe: {err}");
        }
        Some(sub)
    }
}

/// Decode a `{ height }` map payload. A missing / non-numeric `height`
/// degrades to `0.0` (keyboard treated as hidden) rather than wedging
/// the subscription.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32", test))]
fn decode_payload(value: WhiskerValue) -> Option<f64> {
    let WhiskerValue::Map(fields) = value else {
        return None;
    };
    let height = match fields.get("height") {
        Some(WhiskerValue::Float(v)) => *v,
        Some(WhiskerValue::Int(v)) => *v as f64,
        _ => 0.0,
    };
    // Guard against a stray negative from a mid-animation frame.
    Some(height.max(0.0))
}

/// Empty visual schema paired with the service-only Web keyboard Host module.
#[doc(hidden)]
pub fn __whisker_element_module_definition() -> whisker::ElementModuleDefinition {
    whisker::ElementModuleDefinition::new(
        env!("CARGO_PKG_NAME"),
        std::iter::empty::<whisker::ElementProviderMetadata>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn decode_reads_float_and_int_height() {
        let mut m = BTreeMap::new();
        m.insert("height".to_string(), WhiskerValue::Float(291.0));
        assert_eq!(decode_payload(WhiskerValue::Map(m)), Some(291.0));

        let mut m = BTreeMap::new();
        m.insert("height".to_string(), WhiskerValue::Int(216));
        assert_eq!(decode_payload(WhiskerValue::Map(m)), Some(216.0));
    }

    #[test]
    fn decode_missing_height_is_zero() {
        let m = BTreeMap::new();
        assert_eq!(decode_payload(WhiskerValue::Map(m)), Some(0.0));
    }

    #[test]
    fn decode_clamps_negative_to_zero() {
        let mut m = BTreeMap::new();
        m.insert("height".to_string(), WhiskerValue::Float(-5.0));
        assert_eq!(decode_payload(WhiskerValue::Map(m)), Some(0.0));
    }

    #[test]
    fn decode_non_map_is_none() {
        assert_eq!(decode_payload(WhiskerValue::Null), None);
        assert_eq!(decode_payload(WhiskerValue::Float(1.0)), None);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[test]
    fn desktop_fallback_stays_zero() {
        dismiss();
        let runtime =
            whisker::runtime::RuntimeContext::new(whisker::runtime::RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| assert_eq!(keyboard_height().get(), 0.0));
    }
}
