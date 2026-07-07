# whisker-keyboard

The on-screen keyboard as a reactive resource for [Whisker](https://github.com/whiskerrs/whisker) apps.

Two capabilities, routed through one native `Keyboard` module:

- **`keyboard_height() -> ReadSignal<f64>`** — the keyboard's current
  overlap from the bottom of the screen (points on iOS, dp on Android),
  `0.0` when hidden. Pad or scroll a container by this value so a focused
  input isn't covered. Whisker's analogue of Flutter's
  `MediaQuery.viewInsets.bottom` / React Native's keyboard-frame events.

- **`dismiss()`** — a **real global unfocus** (not merely "hide the
  keyboard"): iOS `resignFirstResponder` via the key window's
  `endEditing(true)`, Android `clearFocus()` on the focused view + IME
  hide. Removing focus is what stops a **hardware keyboard** from
  continuing to type into an input that has scrolled or navigated off
  screen.

```rust
use whisker::prelude::*;
use whisker_keyboard::keyboard_height;

#[component]
fn form() -> Element {
    let kb = keyboard_height();
    let pad = move || format!("padding-bottom: {}px;", kb.get());
    render! {
        scroll_view(style: pad()) { /* fields … */ }
    }
}
```

Adding this crate as a dependency also links the native module, which is
what lets [`whisker-router`](../whisker-router) release focus on every
navigation (so the keyboard drops crisply as the user leaves a screen).
