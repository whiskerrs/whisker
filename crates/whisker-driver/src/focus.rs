//! Process-global "currently focused element" registry — Whisker's
//! analogue of React Native's `TextInput.State.currentlyFocusedInput()`.
//!
//! A focusable native element (an `<input>`) records itself here when it
//! gains focus and clears itself when it loses focus. Navigation code
//! (`whisker-router`) reads [`focused_element`] so it can blur — or later
//! restore — the *specific* field that was focused, instead of firing a
//! global unfocus. A global unfocus dispatched at navigation time can
//! land late on the native side and resign a field the *incoming* screen
//! has since auto-focused; a targeted blur of the captured departing
//! field cannot (blurring an already-unmounted field is a no-op). This is
//! exactly why React Navigation captures the concrete input and blurs
//! *that* ref rather than calling `Keyboard.dismiss()` on forward pushes.
//!
//! Main-thread only: the reactive/UI world is thread-local, and the cell
//! is only ever touched from focus/blur event handlers and navigation
//! verbs, all of which run on the Lynx TASM thread.

use std::cell::Cell;

use crate::element_ref::ElementRef;

thread_local! {
    static FOCUSED: Cell<Option<ElementRef>> = const { Cell::new(None) };
}

/// Record `el` as the element that currently holds focus. Call from an
/// input's focus handler.
pub fn note_focused(el: ElementRef) {
    FOCUSED.with(|f| f.set(Some(el)));
}

/// Clear the focused element **iff** it is still `el`, so a stale blur
/// (fired after another field already took focus) can't wipe the newer
/// registration. Call from an input's blur handler.
pub fn note_blurred(el: ElementRef) {
    FOCUSED.with(|f| {
        if f.get() == Some(el) {
            f.set(None);
        }
    });
}

/// The element that currently holds focus, if any.
pub fn focused_element() -> Option<ElementRef> {
    FOCUSED.with(Cell::get)
}
