//! `WhiskerValue` — the universal tagged-union value model shared by
//! the whole framework.
//!
//! It is the wire representation for everything that crosses the
//! Rust ⇄ native boundary as data rather than a handle:
//!
//!   - **module function args / returns** (`PlatformModule::invoke`) and
//!     **element command parameters** (`ElementRef::command`) — raw values, with no typed-arg
//!     deserialization at the boundary, and
//!   - **event payloads** — the body a Host hands a tap / touch /
//!     animation handler, delivered to the Rust closure as a
//!     recursive `WhiskerValue` tree.
//!
//! Lives in its own dependency-neutral crate so the protocol, runtime,
//! Desktop Host, Web Host, and FFI driver all use the exact same Rust type.
//! The FFI mirror (`WhiskerValueRaw`) and the
//! `WhiskerValueRaw` ⇄ `WhiskerValue` marshalling stay in
//! `whisker-driver-sys` / `whisker-driver` — this module is pure
//! Rust with no FFI.
//!
//! ## Typed extraction
//!
//! [`WhiskerValue::deserialize_into`] converts a value tree into any
//! `serde::Deserialize` type (the typed event structs in
//! Whisker runtime event types use this). The conversion goes through
//! `serde_json::Value` as a mature, well-tested `Deserializer`
//! rather than hand-rolling one over `WhiskerValue`. The intermediate
//! is in-memory, with no string parse on the path.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};

/// Tagged-union variant set passed between Rust and the platform
/// side — module args/returns and event payloads alike.
///
/// `String`/`Bytes`/`Array`/`Map` are Rust-allocated and dropped on
/// scope exit. Conversion to/from the C `WhiskerValueRaw` form
/// happens at the FFI boundary in `whisker-driver`.
///
/// This enum is intentionally **closed** (not `#[non_exhaustive]`): it
/// models a sealed tagged union mirroring the C `WhiskerValueRaw`
/// discriminator, and the FFI encoder in `whisker-driver` must handle
/// every variant exhaustively. Adding a variant is a deliberate,
/// breaking wire-format change touching both sides of the boundary, so
/// callers may rely on matching it exhaustively.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum WhiskerValue {
    /// Explicit absence of a value.
    #[default]
    Null,
    /// Boolean value.
    Bool(bool),
    /// Signed 64-bit integer.
    Int(i64),
    /// 64-bit floating-point number.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Opaque byte sequence.
    Bytes(Vec<u8>),
    /// Ordered values.
    Array(Vec<WhiskerValue>),
    /// String-keyed map. `BTreeMap` for deterministic iteration
    /// order — important for snapshot tests and the proc-macro
    /// layer that codegen's destructuring patterns based on
    /// key order.
    Map(BTreeMap<String, WhiskerValue>),
    /// Bridge / platform reported failure. Carries a UTF-8
    /// description; the proc macro lifts this into a typed
    /// `Result::Err`.
    Error(String),
}

impl WhiskerValue {
    /// Returns whether this tree contains only transferable data variants.
    ///
    /// `Error` is a module-dispatch result, not property, command, or event
    /// data. Nested errors are rejected as well as a top-level error.
    pub fn is_data(&self) -> bool {
        match self {
            Self::Array(values) => values.iter().all(Self::is_data),
            Self::Map(values) => values.values().all(Self::is_data),
            Self::Error(_) => false,
            Self::Null
            | Self::Bool(_)
            | Self::Int(_)
            | Self::Float(_)
            | Self::String(_)
            | Self::Bytes(_) => true,
        }
    }

    /// Convenience constructor that wraps a `BTreeMap` literal.
    /// Lets test code write
    /// `WhiskerValue::map([("k", WhiskerValue::Int(1))])` without
    /// importing `BTreeMap`.
    pub fn map<I, K>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, WhiskerValue)>,
        K: Into<String>,
    {
        let mut m = BTreeMap::new();
        for (k, v) in entries {
            m.insert(k.into(), v);
        }
        WhiskerValue::Map(m)
    }

    /// Build a named `args` array. This is useful for service-module
    /// payloads that intentionally model positional values inside one
    /// [`WhiskerValue`]. Element commands use their declared single
    /// parameter value directly.
    pub fn args<I>(items: I) -> Self
    where
        I: IntoIterator<Item = WhiskerValue>,
    {
        WhiskerValue::map([("args", WhiskerValue::Array(items.into_iter().collect()))])
    }

    /// Returns the Error message if `self` is the Error variant.
    /// Convenience for callers that want to bail on any failure
    /// without matching the variant manually.
    pub fn as_error(&self) -> Option<&str> {
        if let WhiskerValue::Error(msg) = self {
            Some(msg.as_str())
        } else {
            None
        }
    }

    /// Deserialize this value tree into a typed `T`.
    ///
    /// Used by the typed-event layer: a `Fn(TouchEvent)` handler
    /// receives the event body as a `WhiskerValue`, then
    /// `deserialize_into::<TouchEvent>()` recovers the struct. On
    /// failure (shape mismatch, missing required field) returns
    /// `Err` with a human-readable message — the event binding logs
    /// it together with the raw value rather than silently dropping
    /// the handler call.
    pub fn deserialize_into<T: DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_value(self.to_json()).map_err(|e| e.to_string())
    }

    /// Lower this value tree into a `serde_json::Value`. In-memory
    /// only — the target of [`deserialize_into`](Self::deserialize_into).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            WhiskerValue::Null => J::Null,
            WhiskerValue::Bool(b) => J::Bool(*b),
            WhiskerValue::Int(i) => J::Number((*i).into()),
            WhiskerValue::Float(f) => serde_json::Number::from_f64(*f)
                .map(J::Number)
                .unwrap_or(J::Null),
            WhiskerValue::String(s) => J::String(s.clone()),
            // Events never carry bytes; module values might. Render
            // as an array of byte-valued numbers so the mapping is
            // total and lossless.
            WhiskerValue::Bytes(b) => {
                J::Array(b.iter().map(|x| J::Number((*x as u64).into())).collect())
            }
            WhiskerValue::Array(a) => J::Array(a.iter().map(WhiskerValue::to_json).collect()),
            WhiskerValue::Map(m) => {
                J::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
            // An Error reaching a typed-deserialize target is a
            // dispatch failure; surface its message as a string so a
            // `String`-typed field can still read it.
            WhiskerValue::Error(e) => J::String(e.clone()),
        }
    }

    /// Lift a `serde_json::Value` into a `WhiskerValue`.
    pub fn from_json(v: serde_json::Value) -> WhiskerValue {
        use serde_json::Value as J;
        match v {
            J::Null => WhiskerValue::Null,
            J::Bool(b) => WhiskerValue::Bool(b),
            J::Number(n) => {
                if let Some(i) = n.as_i64() {
                    WhiskerValue::Int(i)
                } else if let Some(f) = n.as_f64() {
                    WhiskerValue::Float(f)
                } else {
                    WhiskerValue::Null
                }
            }
            J::String(s) => WhiskerValue::String(s),
            J::Array(a) => {
                WhiskerValue::Array(a.into_iter().map(WhiskerValue::from_json).collect())
            }
            J::Object(o) => WhiskerValue::Map(
                o.into_iter()
                    .map(|(k, v)| (k, WhiskerValue::from_json(v)))
                    .collect(),
            ),
        }
    }
}

// Deserialize impl: lets a typed event struct hold an arbitrary
// sub-tree (a `data-*` dataset, a custom event's `detail`) as a
// `WhiskerValue` field without leaking `serde_json::Value` into the
// public event API. Mirrors `serde_json::Value`'s own visitor — each
// serde scalar / seq / map maps to the matching variant.
impl<'de> Deserialize<'de> for WhiskerValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WhiskerValueVisitor;

        impl<'de> Visitor<'de> for WhiskerValueVisitor {
            type Value = WhiskerValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON-compatible value")
            }

            fn visit_unit<E>(self) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Null)
            }
            fn visit_none<E>(self) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Null)
            }
            fn visit_some<D>(self, d: D) -> Result<WhiskerValue, D::Error>
            where
                D: Deserializer<'de>,
            {
                Deserialize::deserialize(d)
            }
            fn visit_bool<E>(self, v: bool) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Bool(v))
            }
            fn visit_i64<E>(self, v: i64) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Int(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<WhiskerValue, E> {
                Ok(i64::try_from(v).map_or(WhiskerValue::Float(v as f64), WhiskerValue::Int))
            }
            fn visit_f64<E>(self, v: f64) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Float(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::String(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::String(v))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<WhiskerValue, E> {
                Ok(WhiskerValue::Bytes(v.to_owned()))
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<WhiskerValue, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut out = Vec::new();
                while let Some(item) = seq.next_element()? {
                    out.push(item);
                }
                Ok(WhiskerValue::Array(out))
            }
            fn visit_map<A>(self, mut map: A) -> Result<WhiskerValue, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut out = BTreeMap::new();
                while let Some((k, v)) = map.next_entry::<String, WhiskerValue>()? {
                    out.insert(k, v);
                }
                Ok(WhiskerValue::Map(out))
            }
        }

        deserializer.deserialize_any(WhiskerValueVisitor)
    }
}

/// Failure surface for module function calls.
///
/// Wraps the UTF-8 description the bridge returned via
/// [`WhiskerValue::Error`] (unknown module / missing method /
/// platform-side exception), plus type-mismatch messages a proxy
/// synthesises when the bridge returned an unexpected variant for
/// the declared return type. Implements [`std::error::Error`] so
/// callers can `?`-propagate through `Result` chains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhiskerModuleError(pub String);

impl std::fmt::Display for WhiskerModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WhiskerModuleError {}

impl From<()> for WhiskerValue {
    fn from(_: ()) -> Self {
        WhiskerValue::Null
    }
}
impl From<bool> for WhiskerValue {
    fn from(v: bool) -> Self {
        WhiskerValue::Bool(v)
    }
}
impl From<i32> for WhiskerValue {
    fn from(v: i32) -> Self {
        WhiskerValue::Int(v as i64)
    }
}
impl From<i64> for WhiskerValue {
    fn from(v: i64) -> Self {
        WhiskerValue::Int(v)
    }
}
impl From<u32> for WhiskerValue {
    fn from(v: u32) -> Self {
        WhiskerValue::Int(v as i64)
    }
}
impl From<f32> for WhiskerValue {
    fn from(v: f32) -> Self {
        WhiskerValue::Float(v as f64)
    }
}
impl From<f64> for WhiskerValue {
    fn from(v: f64) -> Self {
        WhiskerValue::Float(v)
    }
}
impl From<String> for WhiskerValue {
    fn from(v: String) -> Self {
        WhiskerValue::String(v)
    }
}
impl From<&str> for WhiskerValue {
    fn from(v: &str) -> Self {
        WhiskerValue::String(v.to_string())
    }
}
impl From<Vec<u8>> for WhiskerValue {
    fn from(v: Vec<u8>) -> Self {
        WhiskerValue::Bytes(v)
    }
}
impl<T> From<Vec<T>> for WhiskerValue
where
    T: Into<WhiskerValue>,
{
    fn from(v: Vec<T>) -> Self {
        WhiskerValue::Array(v.into_iter().map(Into::into).collect())
    }
}

// TryFrom impls support typed wrappers around module function results.

impl TryFrom<WhiskerValue> for () {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Null => Ok(()),
            other => Err(format!("expected Null, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for bool {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Bool(b) => Ok(b),
            other => Err(format!("expected Bool, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for i64 {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Int(i) => Ok(i),
            other => Err(format!("expected Int, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for i32 {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Int(i) => {
                i32::try_from(i).map_err(|_| format!("Int {i} out of range for i32"))
            }
            other => Err(format!("expected Int, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for f64 {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Float(f) => Ok(f),
            // Widen Int → Float so platforms that return integer-valued
            // numbers don't trip up callers asking for f64.
            WhiskerValue::Int(i) => Ok(i as f64),
            other => Err(format!("expected Float, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for f32 {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        f64::try_from(v).map(|f| f as f32)
    }
}

impl TryFrom<WhiskerValue> for String {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::String(s) => Ok(s),
            other => Err(format!("expected String, got {other:?}")),
        }
    }
}

impl TryFrom<WhiskerValue> for Vec<u8> {
    type Error = String;
    fn try_from(v: WhiskerValue) -> Result<Self, Self::Error> {
        match v {
            WhiskerValue::Bytes(b) => Ok(b),
            other => Err(format!("expected Bytes, got {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_validation_rejects_error_at_every_depth() {
        assert!(
            WhiskerValue::map([("items", WhiskerValue::Array(vec![WhiskerValue::Int(1)]),)])
                .is_data()
        );
        assert!(
            !WhiskerValue::map([(
                "items",
                WhiskerValue::Array(vec![WhiskerValue::Error("failed".into())]),
            )])
            .is_data()
        );
    }
}
