//! Runtime-local "currently focused element" registry — Whisker's
//! analogue of React Native's `TextInput.State.currentlyFocusedInput()`.
//!
//! A focusable native element (an `<input>`) records itself here when it
//! gains focus and clears itself on blur or unmount. Navigation code
//! (`whisker-router`) reads [`focused_element`] so it can blur — or later
//! restore — the *specific* field that was focused, instead of firing a
//! global unfocus. A global unfocus dispatched at navigation time can
//! land late on the native side and resign a field the *incoming* screen
//! has since auto-focused; a targeted blur of the captured departing
//! field cannot (blurring an already-unmounted field is a no-op). This is
//! exactly why React Navigation captures the concrete input and blurs
//! *that* ref rather than calling `Keyboard.dismiss()` on forward pushes.
//!
//! Main-thread only: the reactive/UI world is runtime-local, and the state
//! is touched by focus/blur events, element cleanup, and navigation
//! verbs on the runtime thread.

use std::cell::Cell;

use crate::element_ref::ElementRef;

#[derive(Default)]
struct FocusState {
    focused: Cell<Option<ElementRef>>,
}

fn state() -> std::rc::Rc<std::cell::RefCell<FocusState>> {
    whisker_runtime::runtime_local::state::<FocusState>()
}

/// Record `el` as the element that currently holds focus. Call from an
/// input's focus handler.
pub fn note_focused(el: ElementRef) {
    if el.is_bound() {
        state().borrow().focused.set(Some(el));
    }
}

/// Clear the focused element **iff** it is still `el`, so a stale blur
/// (fired after another field already took focus) can't wipe the newer
/// registration. Call from an input's blur handler.
pub fn note_blurred(el: ElementRef) {
    let state = state();
    let state = state.borrow();
    if state.focused.get() == Some(el) {
        state.focused.set(None);
    }
}

/// The element that currently holds focus, if any.
pub fn focused_element() -> Option<ElementRef> {
    let state = state();
    let state = state.borrow();
    let focused = state.focused.get().filter(ElementRef::is_bound);
    state.focused.set(focused);
    focused
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker_runtime::{RuntimeContext, RuntimeWakeHandle};

    fn bound_ref() -> ElementRef {
        let element = ElementRef::new();
        element.__bind(whisker_runtime::view::Element::from_raw(42));
        element
    }

    #[test]
    fn focused_element_is_isolated_between_runtime_contexts() {
        let first = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        let second = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        let first_ref = first.enter(bound_ref);
        let second_ref = second.enter(bound_ref);

        first.enter(|| note_focused(first_ref));
        second.enter(|| note_focused(second_ref));
        first.enter(|| note_blurred(first_ref));

        assert_eq!(first.enter(focused_element), None);
        assert_eq!(second.enter(focused_element), Some(second_ref));
    }

    #[test]
    fn unmounting_old_input_preserves_new_focus() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = whisker_runtime::reactive::Owner::new(None);
            owner.with(|| {
                let old = bound_ref();
                let current = bound_ref();
                note_focused(old);
                note_focused(current);
                old.__unbind();
                assert_eq!(focused_element(), Some(current));
                note_focused(old);
                assert_eq!(focused_element(), Some(current));
                current.__unbind();
                assert_eq!(state().borrow().focused.get(), None);
                assert_eq!(focused_element(), None);
            });
            owner.dispose();
        });
    }

    #[test]
    fn disposed_input_is_removed_even_without_an_unmount_notification() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = whisker_runtime::reactive::Owner::new(None);
            let input = owner.with(bound_ref);
            note_focused(input);
            owner.dispose();
            assert_eq!(focused_element(), None);
            assert_eq!(state().borrow().focused.get(), None);
            note_focused(input);
            assert_eq!(state().borrow().focused.get(), None);
        });
    }
}
