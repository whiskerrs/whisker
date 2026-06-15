//! `apply_*` helpers — Static-vs-Dynamic dispatch over [`Signal<T>`]
//! used by every prop-setting code path emitted by the macros.
//!
//! Lives in `whisker_runtime` (not the umbrella `whisker` crate or
//! the proc-macro crate) so the umbrella can re-export it. Both app
//! and module crates depend on the umbrella `whisker` and reach
//! these via `::whisker::runtime::view::apply_styles` / `apply_attr`.
//!
//! The two helpers are intentionally generic over
//! `V: Into<Signal<T>>` plus `T: ToString + Clone + 'static`, so a
//! caller can hand them a `&'static str`, a `String`, a
//! `ReadSignal<String>`, or any other source that
//! `From<...> for Signal<String>` covers. The `Dynamic` branch wraps
//! the read in `effect(...)` so the value re-applies whenever the
//! signal source changes.

use crate::reactive::{effect, Signal};
use crate::view::handle::Element;
use crate::view::renderer::{
    set_attribute, set_attribute_bool, set_attribute_double, set_attribute_int, set_inline_styles,
};

/// Apply an inline-styles value to `h`, picking a static vs reactive
/// code path based on the [`Signal<T>`] variant. The `Dynamic` case
/// wraps the read in an `effect` so the returned
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

/// Apply a named attribute value to `h`. Same Static / Dynamic
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
/// branches. See `crates/whisker-driver-sys/bridge/src/whisker_bridge_common.cc`
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
