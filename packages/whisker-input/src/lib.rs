//! `whisker-input` — native text-input component.
//!
//! **API shape — 2 (Component + ref-bound handle).** A native UI
//! element ([`Input`]) backed by `UITextField` / `UITextView` on iOS
//! and `EditText` on Android, with a Leptos-style **two-way binding**
//! as the headline API plus a typed imperative handle ([`InputRef`])
//! bound on mount via `ref:` for `focus` / `blur` / `clear` /
//! `setValue` / `getValue`.
//!
//! The Lynx tag is `whisker-input:Input` (the crate name is
//! auto-prepended by `#[whisker::module_component]`).
//!
//! ## Usage
//!
//! ### Two-way binding (headline)
//!
//! ```ignore
//! use whisker::prelude::*;
//! use whisker_input::Input;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     let text = RwSignal::new(String::new());
//!     render! {
//!         view(style: "flex-direction: column;") {
//!             Input(
//!                 text: text,
//!                 placeholder: "Type something…",
//!                 style: "height: 44px; font-size: 16px;",
//!             )
//!             // The bound signal updates on every keystroke.
//!             text(value: move || format!("You typed: {}", text.get()))
//!         }
//!     }
//! }
//! ```
//!
//! ### Controlled (escape hatch)
//!
//! Pass `value:` (a read-only [`Signal<String>`]) instead of `text:`
//! and drive writeback yourself from `on_input`:
//!
//! ```ignore
//! let (value, set_value) = signal(String::new());
//! render! {
//!     Input(
//!         value: value,
//!         on_input: move |s: String| set_value.set(s.to_uppercase()),
//!     )
//! }
//! ```
//!
//! ### Multiline
//!
//! ```ignore
//! Input(text: notes, multiline: true, lines: 4,
//!       placeholder: "Notes…",
//!       style: "min-height: 96px;")
//! ```
//!
//! ### Secure (password)
//!
//! ```ignore
//! Input(text: password, secure: true, placeholder: "Password")
//! ```
//!
//! ### Imperative handle
//!
//! ```ignore
//! let field = InputRef::new();
//! render! {
//!     view(style: "flex-direction: row;") {
//!         Input(text: text, input_ref: field.clone())
//!         text(value: "Clear", on_tap: {
//!             let field = field.clone();
//!             move |_| field.clear()
//!         })
//!     }
//! }
//! ```
//!
//! ## Props
//!
//! | Prop               | Type                                  | Default       | Description |
//! |--------------------|---------------------------------------|---------------|-------------|
//! | `text`             | `RwSignal<String>`                    | —             | Two-way bound value. Updates on every keystroke. |
//! | `value`            | `Signal<String>`                      | —             | Controlled read (escape hatch; ignored if `text` is set). |
//! | `on_input`         | `Fn(String)`                          | —             | Fires every keystroke with the new full text. |
//! | `on_change`        | `Fn(String)`                          | —             | Fires when editing ends / value committed. |
//! | `on_focus`         | `Fn()`                                | —             | Field gained focus. |
//! | `on_blur`          | `Fn()`                                | —             | Field lost focus. |
//! | `on_submit`        | `Fn(String)`                          | —             | Return / done key pressed. |
//! | `placeholder`      | `Signal<String>`                      | `""`          | Placeholder text shown when empty. |
//! | `multiline`        | `bool`                                | `false`       | Single-line field vs multiline area. |
//! | `lines`            | `u32`                                 | unset (`0`)   | Fixed visible line count (multiline only). |
//! | `secure`           | `bool`                                | `false`       | Mask input (password entry). |
//! | `editable`         | `bool`                                | `true`        | Allow editing. |
//! | `auto_focus`       | `bool`                                | `false`       | Focus + raise keyboard on mount. |
//! | `max_length`       | `u32`                                 | unset (`0`)   | Maximum character count. |
//! | `keyboard_type`    | [`KeyboardType`]                      | `Default`     | On-screen keyboard layout. |
//! | `return_key`       | [`ReturnKey`]                         | `Default`     | Return-key label / action. |
//! | `caret_color`      | `Signal<String>`                      | `""`          | Cursor color (CSS color string). |
//! | `placeholder_color`| `Signal<String>`                      | `""`          | Placeholder text color. |
//! | `selection_color`  | `Signal<String>`                      | `""`          | Selection-highlight color. |
//! | `style`            | `Signal<String>`                      | `""`          | Standard Whisker CSS style string. |
//! | `input_ref`        | [`InputRef`]                          | —             | Imperative handle (see [Methods](#methods)). |
//!
//! ## Styling
//!
//! Box layout and **text** styling flow through the standard `style:`
//! CSS cascade — `width` / `height` / `padding` / `border` /
//! `background-color` / `border-radius`, plus the text properties the
//! native field honors: `color`, `font-size`, `font-weight`,
//! `text-align`.
//!
//! The cursor / placeholder / selection colors are **explicit props**
//! ([`caret_color`](InputProps), [`placeholder_color`](InputProps),
//! [`selection_color`](InputProps)) rather than CSS, because Lynx's CSS
//! engine doesn't reach the internals of a custom UI element — the
//! parsed style cascade only sets generic view properties, so these
//! sub-element colors must be passed as attributes the native view
//! reads directly.
//!
//! ## Methods
//!
//! Hold an [`InputRef`], pass `input_ref:` to the component, then:
//!
//! - [`InputRef::focus`] — focus the field + raise the keyboard.
//! - [`InputRef::blur`] — resign focus + dismiss the keyboard.
//! - [`InputRef::clear`] — clear the text to empty.
//! - [`InputRef::set_value`] — replace the text imperatively.
//! - [`InputRef::get_value`] — async read of the current text.
//!   Reliable on iOS; Android result-returning element methods may
//!   require a Lynx fork release (see the method docs).
//!
//! ## Native source
//!
//! Contributors: the matching platform module lives at
//!
//! - iOS: `packages/whisker-input/ios/Sources/WhiskerInput/InputModule.swift`
//!   (view: `InputView.swift`)
//! - Android: `packages/whisker-input/android/src/main/kotlin/rs/whisker/elements/input/InputModule.kt`
//!   (view: `WhiskerInputView.kt`)

use std::rc::Rc;

use whisker::platform_module::WhiskerValue;
use whisker::prelude::*;
use whisker::{ElementRef, RefError, Signal, Style};

// ---------------------------------------------------------------------------
// Event payload
// ---------------------------------------------------------------------------

/// Payload of an input event (`input` / `change` / `submit`).
///
/// The native view dispatches the event with the field's current text
/// under `detail.value`. Both platforms deliver it under `detail`: the
/// Android event reporter wraps a custom event's params there, and the
/// iOS bridge normalizes `LynxCustomEvent`'s `params` key to `detail`
/// (see `whisker_bridge_ios.mm`) so this struct reads one shape on both.
/// Every field is `#[serde(default)]` so a partial / mismatched body
/// degrades to an empty string rather than dropping the handler call
/// (the "the event fired is the primary signal" philosophy `bind_typed`
/// follows).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct InputEvent {
    /// The event body's `detail` dict.
    #[serde(default)]
    pub detail: InputDetail,
}

/// The `detail` of an [`InputEvent`] — carries the field's current
/// text under the `value` key.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[non_exhaustive]
pub struct InputDetail {
    /// The field's current full text.
    #[serde(default)]
    pub value: String,
}

impl InputEvent {
    /// The field's current text — shorthand for `self.detail.value`.
    pub fn value(&self) -> &str {
        &self.detail.value
    }

    /// Take ownership of the field's current text.
    pub fn into_value(self) -> String {
        self.detail.value
    }
}

// ---------------------------------------------------------------------------
// Callback newtype
// ---------------------------------------------------------------------------

/// A cloneable user callback for an [`Input`] event prop.
///
/// Wraps `Rc<dyn Fn(A)>` so it's `Clone` (required: `#[component]`
/// re-clones every prop for the hot-reload remount path) and so a
/// bare closure coerces into it via `Into` at the call site
/// (`on_input: move |s| …`). `A` is `String` for value-carrying
/// events (`on_input` / `on_change` / `on_submit`) and `()` for the
/// bare focus / blur events.
///
/// This is the one deviation from the original `Box<dyn Fn(..)>`
/// design sketch: a `Box<dyn Fn>` is neither `Clone` (the macro needs
/// it) nor `Into`-coercible from a closure through the generated
/// `Optional` setter, so the newtype carries both properties.
#[derive(Clone)]
pub struct InputCallback<A>(Rc<dyn Fn(A) + 'static>);

impl<A> InputCallback<A> {
    /// Invoke the wrapped callback.
    pub fn call(&self, arg: A) {
        (self.0)(arg)
    }
}

impl<A, F: Fn(A) + 'static> From<F> for InputCallback<A> {
    fn from(f: F) -> Self {
        InputCallback(Rc::new(f))
    }
}

/// A cloneable no-argument user callback for an [`Input`] event prop
/// (`on_focus` / `on_blur`). Same rationale as [`InputCallback`] — a
/// `Clone`, `Into`-from-closure wrapper around `Rc<dyn Fn()>`.
#[derive(Clone)]
pub struct InputAction(Rc<dyn Fn() + 'static>);

impl InputAction {
    /// Invoke the wrapped callback.
    pub fn call(&self) {
        (self.0)()
    }
}

impl<F: Fn() + 'static> From<F> for InputAction {
    fn from(f: F) -> Self {
        InputAction(Rc::new(f))
    }
}

// ---------------------------------------------------------------------------
// Keyboard / return-key enums
// ---------------------------------------------------------------------------

/// On-screen keyboard layout for an [`Input`]. Variant wire strings
/// are locked against the native modules' string dispatch.
///
/// `#[non_exhaustive]` so a future keyboard type can be added without
/// breaking exhaustive matches downstream.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum KeyboardType {
    /// `"default"` — the standard full keyboard. The default.
    #[default]
    Default,
    /// `"number"` — numeric keypad (integers).
    Number,
    /// `"decimal"` — numeric keypad with a decimal point.
    Decimal,
    /// `"email"` — keyboard tuned for email entry (`@`, `.`).
    Email,
    /// `"phone"` — telephone-style keypad.
    Phone,
    /// `"url"` — keyboard tuned for URL entry (`/`, `.com`).
    Url,
}

impl KeyboardType {
    /// Canonical wire string the native view dispatches on.
    pub const fn as_attr(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Number => "number",
            Self::Decimal => "decimal",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Url => "url",
        }
    }
}

/// Return-key label / action for an [`Input`]. Variant wire strings
/// are locked against the native modules' string dispatch.
///
/// `#[non_exhaustive]` so a future return-key type can be added
/// without breaking exhaustive matches downstream.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ReturnKey {
    /// `"default"` — the platform's default return key. The default.
    #[default]
    Default,
    /// `"done"` — a "Done" key.
    Done,
    /// `"go"` — a "Go" key.
    Go,
    /// `"next"` — a "Next" key (advance to the next field).
    Next,
    /// `"search"` — a "Search" key.
    Search,
    /// `"send"` — a "Send" key.
    Send,
}

impl ReturnKey {
    /// Canonical wire string the native view dispatches on.
    pub const fn as_attr(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Done => "done",
            Self::Go => "go",
            Self::Next => "next",
            Self::Search => "search",
            Self::Send => "send",
        }
    }
}

// ---------------------------------------------------------------------------
// Imperative handle
// ---------------------------------------------------------------------------

/// Typed imperative handle for a mounted [`Input`].
///
/// Wraps the framework-internal `ElementRef` bound on mount when
/// passed as the component's `input_ref:` prop. Methods dispatch the
/// matching platform UI method through `ElementRef::invoke` /
/// `invoke_typed`. The fire-and-forget methods swallow "not mounted"
/// / platform errors — these are UI controls; use [`InputRef::get_value`]
/// (which returns a [`RefError`]) when you need to inspect failures.
///
/// `Clone` produces a shared handle (same backing arena slot), so the
/// same handle can drive multiple event closures.
#[derive(Clone)]
pub struct InputRef {
    r: ElementRef,
}

impl InputRef {
    /// Allocate a fresh, unbound handle. Pass it to the component's
    /// `input_ref:` prop in `render!` to bind it on mount.
    pub fn new() -> Self {
        Self {
            r: ElementRef::new(),
        }
    }

    /// The underlying `ElementRef`. Framework-internal — the [`input`]
    /// component reads it to wire the element's `ref:`. App code holds
    /// the `InputRef` and calls the methods below.
    #[doc(hidden)]
    pub fn r(&self) -> ElementRef {
        self.r
    }

    /// Focus the field and raise the keyboard. No-op if the element
    /// isn't mounted yet.
    pub fn focus(&self) {
        let _ = self.r.invoke("focus", WhiskerValue::Null);
    }

    /// Resign focus and dismiss the keyboard. No-op if the element
    /// isn't mounted or isn't focused.
    pub fn blur(&self) {
        let _ = self.r.invoke("blur", WhiskerValue::Null);
    }

    /// Clear the text to empty. No-op if the element isn't mounted.
    ///
    /// Note: this does NOT write back through a bound `text` signal —
    /// the native view clears its own text, and the resulting `input`
    /// event (if the platform emits one) drives the signal update.
    /// For a guaranteed signal update prefer `text.set(String::new())`.
    pub fn clear(&self) {
        let _ = self.r.invoke("clear", WhiskerValue::Null);
    }

    /// Replace the field's text imperatively. No-op if the element
    /// isn't mounted.
    ///
    /// The native view treats this the same as an external `value`
    /// change: it only re-sets its text when `v` differs from what it
    /// currently shows, so the cursor doesn't jump on a no-op write.
    pub fn set_value(&self, v: &str) {
        let _ = self.r.invoke(
            "setValue",
            WhiskerValue::map([("value", WhiskerValue::String(v.to_string()))]),
        );
    }

    /// Async read of the field's current text.
    ///
    /// **iOS:** reliable — the result arrives over Lynx's UI-method
    /// callback. **Android:** result-returning custom element methods
    /// may require a Lynx fork release (per repo notes — `componentAt`
    /// / result-method plumbing is iOS-only-compiled upstream). On an
    /// unforked Android runtime this resolves to
    /// [`RefError::DispatchFailed`] or `NotBound`; prefer reading a
    /// bound `text` signal there.
    pub async fn get_value(&self) -> Result<String, RefError> {
        self.r
            .invoke_typed::<GetValueResult>("getValue", WhiskerValue::Null)
            .await
            .map(|r| r.value)
    }
}

impl Default for InputRef {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode target for `getValue` — the native side returns
/// `{ "value": "<text>" }`; [`InputRef::get_value`] unwraps it to the
/// bare `String`.
#[derive(Debug, Default, serde::Deserialize)]
struct GetValueResult {
    #[serde(default)]
    value: String,
}

// ---------------------------------------------------------------------------
// Inner native binding — the thin element.
//
// `value` / `placeholder` / `…color` are reactive `Signal<String>`
// attrs (kebab-cased: `placeholder-color`, `caret-color`,
// `selection-color`). The bool / number / enum props are passed as
// pre-stringified `Signal<String>` attrs ("true" / "false", a decimal
// string, the enum's `as_attr`) so the macro's `apply_attr`
// stringifies them uniformly and the native view reads one stable
// string form. `on_*: InputEvent` props are typed events wired via
// `bind_typed`. Crate-internal — only the outer `input` component
// uses it; not part of the public doc surface.
// ---------------------------------------------------------------------------

#[doc(hidden)]
#[whisker::module_component("Input")]
pub fn native_input(
    value: Signal<String>,
    placeholder: Signal<String>,
    placeholder_color: Signal<String>,
    caret_color: Signal<String>,
    selection_color: Signal<String>,
    multiline: Signal<String>,
    lines: Signal<String>,
    secure: Signal<String>,
    editable: Signal<String>,
    auto_focus: Signal<String>,
    max_length: Signal<String>,
    keyboard_type: Signal<String>,
    return_key: Signal<String>,
    style: Style,
    on_input: InputEvent,
    on_change: InputEvent,
    on_focus: InputEvent,
    on_blur: InputEvent,
    on_submit: InputEvent,
) {
}

// ---------------------------------------------------------------------------
// Public ergonomic component.
// ---------------------------------------------------------------------------

/// `whisker-input:Input` — a native text field with Leptos-style
/// two-way binding.
///
/// See the [crate docs](crate) for usage, the full prop table, and
/// styling notes.
#[allow(clippy::too_many_arguments)]
#[component]
pub fn input(
    /// Two-way bound value (headline API). Updates on every keystroke.
    text: Option<RwSignal<String>>,
    /// Controlled read (escape hatch). Ignored when `text` is set.
    value: Option<Signal<String>>,
    /// Fires every keystroke with the new full text.
    on_input: Option<InputCallback<String>>,
    /// Fires when editing ends / the value is committed.
    on_change: Option<InputCallback<String>>,
    /// Field gained focus.
    on_focus: Option<InputAction>,
    /// Field lost focus.
    on_blur: Option<InputAction>,
    /// Return / done key pressed; carries the current text.
    on_submit: Option<InputCallback<String>>,
    /// Placeholder text shown when the field is empty.
    placeholder: Option<Signal<String>>,
    /// Multiline area vs single-line field.
    #[prop(default = false)]
    multiline: bool,
    /// Fixed visible line count (multiline only).
    lines: Option<u32>,
    /// Mask input (password entry).
    #[prop(default = false)]
    secure: bool,
    /// Allow editing.
    #[prop(default = true)]
    editable: bool,
    /// Focus + raise the keyboard on mount.
    #[prop(default = false)]
    auto_focus: bool,
    /// Maximum character count.
    max_length: Option<u32>,
    /// On-screen keyboard layout.
    #[prop(default = KeyboardType::Default)]
    keyboard_type: KeyboardType,
    /// Return-key label / action.
    #[prop(default = ReturnKey::Default)]
    return_key: ReturnKey,
    /// Cursor color (CSS color string).
    caret_color: Option<Signal<String>>,
    /// Placeholder text color (CSS color string).
    placeholder_color: Option<Signal<String>>,
    /// Selection-highlight color (CSS color string).
    selection_color: Option<Signal<String>>,
    /// Standard Whisker style. Accepts a `Css` builder, a raw string,
    /// or a reactive signal of either — same as a built-in element's
    /// `style:`.
    style: Option<Style>,
    /// Imperative handle ([`InputRef`]).
    input_ref: Option<InputRef>,
) -> Element {
    // ----- Effective displayed value (reactive) ------------------------
    //
    // Priority: `text` (two-way) → `value` (controlled) → empty. Feed
    // it down to `NativeInput`'s `value` prop as a `Signal::Dynamic`
    // so an external `text.set(...)` / a controlled `value` change
    // re-applies natively.
    //
    // We do NOT guard cursor-jump / echo here: the native view only
    // re-sets its displayed text when the incoming `value` differs
    // from what it currently shows, so feeding the round-tripped value
    // straight back down is safe.
    //
    // `text` is `Option<RwSignal<String>>` — `RwSignal` is `Copy`, so
    // the whole `Option` is `Copy` and freely usable in every closure
    // below. The other props (`value`, the callbacks) are `Clone`
    // (Rc-backed / signal handles), so we clone them into each closure
    // rather than moving them out of the `#[component]` re-invokable
    // body.
    let value_prop: Signal<String> = {
        let value = value.clone();
        Signal::Dynamic(computed(move || {
            if let Some(t) = text {
                t.get()
            } else if let Some(v) = &value {
                v.get()
            } else {
                String::new()
            }
        }))
    };

    // ----- Event wiring -----------------------------------------------
    //
    // on_input: update the bound `text` signal FIRST (so reads inside
    // the user callback already see the new value), then call the
    // user's `on_input`.
    let on_input_cb = {
        let on_input = on_input.clone();
        move |ev: InputEvent| {
            let new = ev.into_value();
            if let Some(t) = text {
                t.set(new.clone());
            }
            if let Some(cb) = &on_input {
                cb.call(new);
            }
        }
    };
    let on_change_cb = {
        let on_change = on_change.clone();
        move |ev: InputEvent| {
            if let Some(cb) = &on_change {
                cb.call(ev.into_value());
            }
        }
    };
    let on_focus_cb = {
        let on_focus = on_focus.clone();
        move |_ev: InputEvent| {
            if let Some(cb) = &on_focus {
                cb.call();
            }
        }
    };
    let on_blur_cb = {
        let on_blur = on_blur.clone();
        move |_ev: InputEvent| {
            if let Some(cb) = &on_blur {
                cb.call();
            }
        }
    };
    let on_submit_cb = {
        let on_submit = on_submit.clone();
        move |ev: InputEvent| {
            if let Some(cb) = &on_submit {
                cb.call(ev.into_value());
            }
        }
    };

    // ----- Pass-through attrs (None → sensible default) ----------------
    let placeholder_prop: Signal<String> = placeholder.clone().unwrap_or_default();
    let caret_color_prop: Signal<String> = caret_color.clone().unwrap_or_default();
    let placeholder_color_prop: Signal<String> = placeholder_color.clone().unwrap_or_default();
    let selection_color_prop: Signal<String> = selection_color.clone().unwrap_or_default();
    let style_prop: Style = style.clone().unwrap_or_default();

    let multiline_attr = bool_attr(multiline);
    let secure_attr = bool_attr(secure);
    let editable_attr = bool_attr(editable);
    let auto_focus_attr = bool_attr(auto_focus);
    // `0` is the "unset" sentinel for `lines` / `max_length`.
    let lines_attr = lines.unwrap_or(0).to_string();
    let max_length_attr = max_length.unwrap_or(0).to_string();
    let keyboard_type_attr = keyboard_type.as_attr().to_string();
    let return_key_attr = return_key.as_attr().to_string();

    // ----- Imperative handle: forward its ElementRef as `ref:` ---------
    let element_ref = input_ref.as_ref().map(|h| h.r());

    let mut builder = NativeInput::builder()
        .value(value_prop)
        .placeholder(placeholder_prop)
        .placeholder_color(placeholder_color_prop)
        .caret_color(caret_color_prop)
        .selection_color(selection_color_prop)
        .multiline(multiline_attr)
        .lines(lines_attr)
        .secure(secure_attr)
        .editable(editable_attr)
        .auto_focus(auto_focus_attr)
        .max_length(max_length_attr)
        .keyboard_type(keyboard_type_attr)
        .return_key(return_key_attr)
        .style(style_prop)
        .on_input(on_input_cb)
        .on_change(on_change_cb)
        .on_focus(on_focus_cb)
        .on_blur(on_blur_cb)
        .on_submit(on_submit_cb);

    if let Some(r) = element_ref {
        builder = builder.with_ref(r);
    }

    NativeInput(builder.build())
}

/// `true` / `false` wire string for a bool attr.
fn bool_attr(b: bool) -> String {
    if b { "true" } else { "false" }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_type_wire_strings() {
        assert_eq!(KeyboardType::Default.as_attr(), "default");
        assert_eq!(KeyboardType::Number.as_attr(), "number");
        assert_eq!(KeyboardType::Decimal.as_attr(), "decimal");
        assert_eq!(KeyboardType::Email.as_attr(), "email");
        assert_eq!(KeyboardType::Phone.as_attr(), "phone");
        assert_eq!(KeyboardType::Url.as_attr(), "url");
    }

    #[test]
    fn return_key_wire_strings() {
        assert_eq!(ReturnKey::Default.as_attr(), "default");
        assert_eq!(ReturnKey::Done.as_attr(), "done");
        assert_eq!(ReturnKey::Go.as_attr(), "go");
        assert_eq!(ReturnKey::Next.as_attr(), "next");
        assert_eq!(ReturnKey::Search.as_attr(), "search");
        assert_eq!(ReturnKey::Send.as_attr(), "send");
    }

    #[test]
    fn enum_defaults() {
        assert_eq!(KeyboardType::default(), KeyboardType::Default);
        assert_eq!(ReturnKey::default(), ReturnKey::Default);
    }

    #[test]
    fn bool_attr_strings() {
        assert_eq!(bool_attr(true), "true");
        assert_eq!(bool_attr(false), "false");
    }

    #[test]
    fn input_event_deserializes_detail_value() {
        // Mirrors the native event body: { detail: { value: "<text>" } }.
        let v = WhiskerValue::map([(
            "detail",
            WhiskerValue::map([("value", WhiskerValue::String("hello".into()))]),
        )]);
        let ev: InputEvent = v.deserialize_into().expect("deserialize InputEvent");
        assert_eq!(ev.value(), "hello");
        assert_eq!(ev.into_value(), "hello");
    }

    #[test]
    fn input_event_empty_body_defaults_to_empty() {
        // focus / blur carry no value: the native side emits an empty
        // (or `detail`-less) body. An empty map deserializes cleanly to
        // an empty value via the `#[serde(default)]` on `detail`.
        let empty: [(&str, WhiskerValue); 0] = [];
        let ev: InputEvent = WhiskerValue::map(empty)
            .deserialize_into()
            .expect("empty map defaults");
        assert_eq!(ev.value(), "");
    }

    #[test]
    fn input_event_default_is_empty() {
        // serde refuses to build a struct from a bare `null`, so for a
        // truly null body `bind_typed` falls back to `E::default()` —
        // which must be the empty-value event.
        assert_eq!(InputEvent::default().value(), "");
    }

    #[test]
    fn get_value_result_unwraps_value() {
        let v = WhiskerValue::map([("value", WhiskerValue::String("abc".into()))]);
        let r: GetValueResult = v.deserialize_into().expect("deserialize getValue");
        assert_eq!(r.value, "abc");
    }
}
