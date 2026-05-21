//! Counter — the canonical Phase 6.5a (Leptos-style) Whisker example.
//!
//! This crate is intentionally minimal: one signal, one effect (via
//! `render!`'s `{count.get()}` interpolation), three buttons. The
//! goal is to demonstrate the full reactive surface — `signal`,
//! `#[component]`, `render!`, event handlers, the `Show` control
//! flow component — in the smallest possible footprint.
//!
//! ## Running this in tests
//!
//! Production deployment (`whisker run`) still flows through the
//! pre-A3 bootstrap that expects an `Element` value tree, so the
//! counter currently runs only against the in-memory testing
//! renderer (`tests/counter_renders.rs`). Step 5b of #11 rewrites
//! the bootstrap so this same crate becomes a deployable app.
//!
//! ## Reading order
//!
//! 1. [`AppState`] — the shared, app-wide reactive state. One
//!    `RwSignal<i32>` for the count.
//! 2. [`counter`] — the component. Reads the count, renders a label
//!    plus three buttons, gates an "over 10!" message with `Show`.
//! 3. [`render_app`] — entry that mounts the counter as the root.

use whisker::prelude::*;

/// App-wide state. A single signal is enough for the demo; bigger
/// apps would group several signals (or derived `ReadSignal`s built
/// with `computed()`) into a struct like this and pass it by `Copy` into
/// child components.
#[derive(Copy, Clone)]
pub struct AppState {
    pub count: RwSignal<i32>,
}

/// Counter component. Returns the root view element handle.
///
/// The macro generates:
/// - `view` element for the wrapper, with one `effect` per dynamic
///   attribute / interpolation.
/// - `text { "Count: " {count} }` produces two raw_text elements —
///   one static, one driven by an effect that re-runs every time
///   `count` changes.
/// - Three buttons whose `on_tap` handlers update the signal. Writes
///   are batched within each handler so multi-update handlers only
///   trigger one re-render.
/// - `Show` toggles a celebratory message in/out depending on a
///   computed value that derives from the same signal.
#[component]
pub fn counter(state: AppState) -> whisker::runtime::view::Element {
    let big_enough = computed(move || state.count.get() > 10);

    render! {
        view(style: "display: flex; flex-direction: column; gap: 12px; padding: 20px;") {
            text(style: "font-size: 32px; font-weight: 700;") {
                text(value: format!("Count: {}", state.count.get()))
            }

            view(style: "display: flex; flex-direction: row; gap: 8px;") {
                view(
                    style: "padding: 8px 16px; background: #e5e7eb; border-radius: 6px;",
                    on_tap: move || state.count.update(|n| *n -= 1),
                ) {
                    text(value: "-1")
                }

                view(
                    style: "padding: 8px 16px; background: #e5e7eb; border-radius: 6px;",
                    on_tap: move || state.count.set(0),
                ) {
                    text(value: "reset")
                }

                view(
                    style: "padding: 8px 16px; background: #3b82f6; color: white; border-radius: 6px;",
                    on_tap: move || state.count.update(|n| *n += 1),
                ) {
                    text(value: "+1")
                }
            }

            Show(when: move || big_enough.get()) {
                text(style: "color: #16a34a; font-weight: 600;") {
                    text(value: "You went over 10!")
                }
            }
        }
    }
}

/// Mount the root component. Creates the shared state and invokes
/// the counter component. The returned handle is what a host would
/// hand to `set_root(...)` once deployed.
pub fn render_app() -> whisker::runtime::view::Element {
    let state = AppState {
        count: RwSignal::new(0),
    };
    render! { counter(state: state) }
}
