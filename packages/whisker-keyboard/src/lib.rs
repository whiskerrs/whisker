//! `whisker-keyboard` — the on-screen keyboard as a reactive resource.
//!
//! Two capabilities, both routed through one native `Keyboard` module:
//!
//! - [`keyboard_height`] — a process-global
//!   `ReadSignal<f64>` carrying the keyboard's current overlap from the
//!   bottom of the screen (points on iOS, dp on Android), `0.0` when
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
//!   or navigated off screen (on Android, `hideSoftInputFromWindow`
//!   alone leaves the field focused). Mirrors React Native's
//!   `Keyboard.dismiss()` / Flutter's `FocusManager.primaryFocus.unfocus()`.
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
//!         scroll_view(style: pad()) { /* fields … */ }
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

use std::sync::OnceLock;

use whisker::module;
use whisker::{ArcRwSignal, ArcWriteSignal, Owner, ReadSignal, WhiskerValue};

/// Dismiss the keyboard by releasing focus globally.
///
/// This is a **real unfocus**: iOS `endEditing(true)` on the key
/// window (resign first responder), Android `clearFocus()` on the
/// focused view + IME hide. A no-op when nothing is focused. Safe to
/// call from any Whisker event handler; the native side marshals the
/// UIKit / Android View work to the main thread.
pub fn dismiss() {
    // Fire-and-forget; an unregistered module (app didn't add
    // whisker-keyboard's native half) surfaces as a swallowed
    // `WhiskerValue::Error`, degrading to a no-op.
    let _ = module!("Keyboard").invoke("dismiss", vec![]);
}

/// Reactive accessor for the on-screen keyboard's current height —
/// the overlap from the bottom of the screen in points (iOS) / dp
/// (Android), `0.0` when the keyboard is hidden.
///
/// All calls share one process-global signal (see [`safe_area_insets`
/// in `whisker-safe-area`] for the identical pattern and the
/// detached-root minting rationale). The first call wires the native
/// subscription; later calls are free. The value stays live for the
/// process lifetime.
///
/// **Must be called from the main thread.** The reactive runtime is
/// thread-local.
///
/// [`safe_area_insets` in `whisker-safe-area`]: https://docs.rs/whisker-safe-area
pub fn keyboard_height() -> ReadSignal<f64> {
    install();
    SLOT.get().expect("install() ran above").read.inner
}

// ---- Internals -------------------------------------------------------------

struct Slot {
    read: MainThreadOnly<ReadSignal<f64>>,
    #[allow(dead_code)]
    write: MainThreadOnly<ArcWriteSignal<f64>>,
}

/// One-shot install of the global signal + native subscription.
/// Idempotent. The `Copy` arena handle callers receive is minted once
/// under a never-disposed [`Owner::detached_root`] so a transient
/// per-route / per-component scope can't free it out from under a
/// surviving reader (the footgun documented at length in
/// `whisker-safe-area`).
fn install() {
    SLOT.get_or_init(|| {
        let (read, write) = ArcRwSignal::new(0.0_f64).split();
        subscribe_to_native(MainThreadOnly {
            inner: write.clone(),
        });
        let root = Owner::detached_root();
        let read_handle: ReadSignal<f64> = root.with(|| read.into());
        Slot {
            read: MainThreadOnly { inner: read_handle },
            write: MainThreadOnly { inner: write },
        }
    });
}

/// Wire the global signal to the native module's `keyboardChanged`
/// event. The subscription is intentionally leaked — the signal lives
/// for the process lifetime.
fn subscribe_to_native(writer: MainThreadOnly<ArcWriteSignal<f64>>) {
    let module = module!("Keyboard");
    let sub = module.on_event("keyboardChanged", move |payload| {
        if let Some(height) = decode_payload(payload) {
            let w = &writer;
            w.inner.set(height);
        }
    });
    if let Some(err) = sub.error() {
        eprintln!("[whisker-keyboard] failed to subscribe: {err}");
    }
    std::mem::forget(sub);
}

/// Decode a `{ height }` map payload. A missing / non-numeric `height`
/// degrades to `0.0` (keyboard treated as hidden) rather than wedging
/// the subscription.
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

static SLOT: OnceLock<Slot> = OnceLock::new();

/// Locally-scoped wrapper asserting main-thread-only access to
/// `inner`. Same pattern (and safety contract) as
/// `whisker-safe-area`'s `MainThreadOnly`: every access path runs on
/// the Lynx TASM thread by contract.
#[derive(Copy, Clone)]
struct MainThreadOnly<T> {
    inner: T,
}
// Safety: signal read (`keyboard_height`) and write (the `on_event`
// callback) both run on the Lynx TASM thread by contract. Misuse
// would corrupt the reactive arena — same risk as touching any signal
// API from a worker thread.
unsafe impl<T> Send for MainThreadOnly<T> {}
unsafe impl<T> Sync for MainThreadOnly<T> {}

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
}
