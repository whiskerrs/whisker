//! Keyboard focus handling across navigations — a faithful port of
//! React Navigation's Stack Navigator `useKeyboardManager`
//! (`@react-navigation/stack`, `utils/useKeyboardManager`).
//!
//! ## Why not a global dismiss
//!
//! The obvious implementation — call [`whisker_keyboard::dismiss`] on
//! every navigation — is what this crate used to do, and it is *more*
//! aggressive than React Navigation. A global unfocus dispatched at
//! navigation time is fire-and-forget on the native side; it can land
//! *after* the screen being entered has already auto-focused its own
//! input, resigning it (the "focus flashes then drops ~0.5s in" bug).
//!
//! React Navigation avoids this by capturing the concrete focused input
//! (`TextInput.State.currentlyFocusedInput()`) and blurring *that ref*,
//! never a blanket dismiss on a forward push. Whisker mirrors it: the
//! focused-element registry ([`whisker::focus`]) is the
//! `currentlyFocusedInput()` analogue, and we blur/refocus that exact
//! [`ElementRef`]. Blurring a field that has since unmounted (the screen
//! we left) is a no-op, so a late-landing blur can never steal the
//! incoming screen's focus.
//!
//! ## The three hooks (named after the RN originals)
//!
//! - [`on_page_change_start`] — a back gesture began. Blur the focused
//!   field and remember it, so a cancelled gesture can restore it.
//! - [`on_page_change_cancel`] — the gesture was released below the
//!   commit threshold. Refocus the remembered field. If the whole
//!   interaction was shorter than [`KEYBOARD_FLASH_GUARD`], defer the
//!   refocus: the platform force-hides the keyboard for a beat after an
//!   interactive dismiss begins, so an immediate refocus would only
//!   flash it.
//! - [`on_page_change_confirm`] — a navigation committed. For a
//!   programmatic verb, blur whatever is focused (targeted). For a
//!   committed gesture, the remembered field stays blurred.

use std::cell::Cell;
use std::time::{Duration, Instant};

use whisker::{ElementRef, WhiskerValue, run_blocking, spawn_local};

/// Below this interaction length, defer a cancel's refocus to dodge the
/// keyboard-dismiss tail. Matches RN's 100ms guard.
const KEYBOARD_FLASH_GUARD: Duration = Duration::from_millis(100);

thread_local! {
    /// The field focused when the current back gesture began — restored
    /// on cancel, dropped on commit.
    static REMEMBERED: Cell<Option<ElementRef>> = const { Cell::new(None) };
    /// When that gesture began, for the flash guard.
    static STARTED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Blur a specific field. A no-op when the element is unmounted, which is
/// exactly why targeting the captured departing field is race-free.
fn blur(el: ElementRef) {
    let _ = el.invoke("blur", WhiskerValue::Null);
}

/// Focus a specific field (raises the keyboard).
fn focus(el: ElementRef) {
    let _ = el.invoke("focus", WhiskerValue::Null);
}

/// A back gesture began: blur the currently-focused field and remember it
/// so [`on_page_change_cancel`] can put it back. (RN `onPageChangeStart`.)
pub(crate) fn on_page_change_start() {
    // Idempotent: a gesture can signal "start" more than once (Android
    // retries `begin()` on the first progress event when `backStarted`
    // was dropped). Only the first capture of a gesture counts, else a
    // second call would overwrite the remembered field with `None`.
    if STARTED_AT.with(|s| s.get().is_some()) {
        return;
    }
    let current = whisker::focus::focused_element();
    REMEMBERED.with(|r| r.set(current));
    STARTED_AT.with(|s| s.set(Some(Instant::now())));
    if let Some(el) = current {
        blur(el);
    }
}

/// The gesture was cancelled: refocus the remembered field, deferring
/// briefly for a too-fast interaction so the keyboard doesn't flash.
/// (RN `onPageChangeCancel`.)
pub(crate) fn on_page_change_cancel() {
    let Some(el) = REMEMBERED.with(Cell::take) else {
        return;
    };
    let elapsed = STARTED_AT
        .with(Cell::take)
        .map(|t| t.elapsed())
        .unwrap_or(KEYBOARD_FLASH_GUARD);
    if elapsed >= KEYBOARD_FLASH_GUARD {
        focus(el);
    } else {
        let wait = KEYBOARD_FLASH_GUARD - elapsed;
        spawn_local(async move {
            run_blocking(move || std::thread::sleep(wait)).await;
            focus(el);
        });
    }
}

/// A navigation committed. `gesture` marks an interactive back gesture
/// (vs a programmatic verb). (RN `onPageChangeConfirm` with `closing`
/// always true here — the verb already happened.)
pub(crate) fn on_page_change_confirm(gesture: bool) {
    if gesture {
        // The interactive back committed. The field captured at gesture
        // start stays blurred; just release it.
        if let Some(el) = REMEMBERED.with(Cell::take) {
            blur(el);
        }
        STARTED_AT.with(|s| s.set(None));
    } else {
        // Programmatic navigation: blur the currently-focused field, if
        // any. Targeted, so a late-landing blur hits only the departing
        // field — never the incoming screen's freshly-focused input.
        if let Some(el) = whisker::focus::focused_element() {
            blur(el);
        }
    }
}
