//! `apply_*` helpers — Stored-vs-Dynamic dispatch over [`Signal<T>`]
//! used by every prop-setting code path emitted by the macros.
//!
//! They are generic over `V: Into<Signal<T>>` plus
//! `T: ToString + Clone + 'static`, so a caller can hand them a
//! `&'static str`, a `String`, a `ReadSignal<String>`, or anything else
//! `From<...> for Signal<String>` covers. The `Dynamic` branch wraps
//! the read in `effect(...)` so the value re-applies whenever the
//! signal source changes.

use crate::event::Dataset;
use crate::reactive::{Signal, effect};
use crate::view::handle::Element;
use crate::view::renderer::{
    set_accessibility, set_attribute, set_attribute_bool, set_attribute_double, set_attribute_int,
    set_dataset, set_element_id, set_text_max_lines,
};
use whisker_protocol::Accessibility;

/// Applies a reactive framework-level element identifier.
pub fn apply_element_id<V>(handle: Element, value: V)
where
    V: Into<Signal<String>>,
{
    match value.into() {
        Signal::Stored(value) => value.with(|value| set_element_id(handle, value.clone())),
        Signal::Dynamic(value) => {
            effect(move || set_element_id(handle, value.get()));
        }
    }
}

/// Applies a reactive structured dataset.
pub fn apply_dataset<V>(handle: Element, value: V)
where
    V: Into<Signal<Dataset>>,
{
    match value.into() {
        Signal::Stored(value) => value.with(|value| set_dataset(handle, value.clone())),
        Signal::Dynamic(value) => {
            effect(move || set_dataset(handle, value.get()));
        }
    }
}

/// Applies reactive common accessibility semantics.
pub fn apply_accessibility<V>(handle: Element, value: V)
where
    V: Into<Signal<Accessibility>>,
{
    match value.into() {
        Signal::Stored(value) => value.with(|value| set_accessibility(handle, value.clone())),
        Signal::Dynamic(value) => {
            effect(move || set_accessibility(handle, value.get()));
        }
    }
}

/// Applies a reactive plain-text line limit (`0` means unlimited).
pub fn apply_text_max_lines<V>(handle: Element, value: V)
where
    V: Into<Signal<u32>>,
{
    match value.into() {
        Signal::Stored(value) => value.with(|value| set_text_max_lines(handle, *value)),
        Signal::Dynamic(value) => {
            effect(move || set_text_max_lines(handle, value.get()));
        }
    }
}

/// Apply a named attribute value to `h`. Same Stored / Dynamic
/// dispatch as other reactive property helpers.
pub fn apply_attr<V, T>(h: Element, name: &'static str, v: V)
where
    V: ::std::convert::Into<Signal<T>>,
    T: ::std::string::ToString + ::std::clone::Clone + 'static,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute(h, name, &t.to_string())),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute(h, name, &sig.get().to_string()));
        }
    }
}

/// Typed-attribute helpers — use these when the Host-side handler
/// reads the value as anything other than a string. Host's prop
/// dispatch on many UIs (`<list>`, `<scroll-view>`, …) gates
/// branches on `value.IsNumber()` / `value.IsBool()`, so a
/// stringified attr from [`apply_attr`] silently no-ops in those
/// branches. Retained Hosts receive the typed value through the frame protocol.
/// for the bridge-side rationale.
pub fn apply_attr_int<V>(h: Element, name: &'static str, v: V)
where
    V: ::std::convert::Into<Signal<i32>>,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute_int(h, name, i64::from(*t))),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute_int(h, name, i64::from(sig.get())));
        }
    }
}

/// Same as [`apply_attr_int`] but for a signal whose element type
/// isn't already `i32` — maps each read through `to_wire` first.
/// Exists for typed attribute enums (e.g. `PanInterceptDirection`)
/// whose Host-side prop setter is integer-typed but where the Rust
/// API keeps the ergonomic enum type; see [`apply_attr_int`]'s doc
/// comment for why the plain string [`apply_attr`] path silently
/// no-ops for these.
pub fn apply_attr_int_mapped<V, T>(h: Element, name: &'static str, v: V, to_wire: fn(T) -> i32)
where
    V: ::std::convert::Into<Signal<T>>,
    T: ::std::marker::Copy + 'static,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute_int(h, name, i64::from(to_wire(*t)))),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute_int(h, name, i64::from(to_wire(sig.get()))));
        }
    }
}

pub fn apply_attr_bool<V>(h: Element, name: &'static str, v: V)
where
    V: ::std::convert::Into<Signal<bool>>,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute_bool(h, name, *t)),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute_bool(h, name, sig.get()));
        }
    }
}

pub fn apply_attr_f64<V>(h: Element, name: &'static str, v: V)
where
    V: ::std::convert::Into<Signal<f64>>,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute_double(h, name, *t)),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute_double(h, name, sig.get()));
        }
    }
}

/// Same as [`apply_attr`] but with an **owned** attribute name, for
/// names computed at the call site (`data-<key>`). The `Dynamic`
/// branch moves the `String` into the `effect` closure so the
/// reactive re-apply keeps the name alive.
pub fn apply_attr_owned<V, T>(h: Element, name: String, v: V)
where
    V: ::std::convert::Into<Signal<T>>,
    T: ::std::string::ToString + ::std::clone::Clone + 'static,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_attribute(h, &name, &t.to_string())),
        Signal::Dynamic(sig) => {
            effect(move || set_attribute(h, &name, &sig.get().to_string()));
        }
    }
}
