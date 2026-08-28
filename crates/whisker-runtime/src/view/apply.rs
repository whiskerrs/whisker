//! `apply_*` helpers — Stored-vs-Dynamic dispatch over [`Signal<T>`]
//! used by every prop-setting code path emitted by the macros.
//!
//! They are generic over `V: Into<Signal<T>>` plus
//! `T: ToString + Clone + 'static`, so a caller can hand them a
//! `&'static str`, a `String`, a `ReadSignal<String>`, or anything else
//! `From<...> for Signal<String>` covers. The `Dynamic` branch wraps
//! the read in `effect(...)` so the value re-applies whenever the
//! signal source changes.

use crate::reactive::{Signal, effect};
use crate::view::handle::Element;
use crate::view::renderer::{
    set_attribute, set_attribute_bool, set_attribute_double, set_attribute_int, set_inline_styles,
};

/// Apply an inline-styles value to `h`, picking a static vs reactive
/// code path based on the [`Signal<T>`] variant. The `Dynamic` case
/// wraps the read in an `effect` so the
/// [`ReadSignal<T>::get`](crate::reactive::ReadSignal::get) call
/// registers the source as a dependency.
pub fn apply_styles<V, T>(h: Element, v: V)
where
    V: ::std::convert::Into<Signal<T>>,
    T: ::std::string::ToString + ::std::clone::Clone + 'static,
{
    match v.into() {
        Signal::Stored(sv) => sv.with(|t| set_inline_styles(h, &t.to_string())),
        Signal::Dynamic(sig) => {
            effect(move || set_inline_styles(h, &sig.get().to_string()));
        }
    }
}

/// Apply a named attribute value to `h`. Same Stored / Dynamic
/// dispatch as [`apply_styles`].
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

/// Typed-attribute helpers — use these when the Lynx-side handler
/// reads the value as anything other than a string. Lynx's prop
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
/// whose Lynx-side prop setter is integer-typed but where the Rust
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
