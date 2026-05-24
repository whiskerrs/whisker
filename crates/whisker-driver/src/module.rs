//! Whisker native module invocation — Rust-side entry point for
//! the C bridge `whisker_bridge_invoke_module*` family
//! (`whisker-driver-sys`).
//!
//! The platform-side dispatch (NSInvocation on iOS, JNI cached
//! `jmethodID` on Android) lives in
//! `crates/whisker-driver-sys/bridge/src/`; this module wraps the
//! raw `WhiskerValueRaw` C-mirror in a typed [`WhiskerValue`]
//! Rust enum and provides ergonomic [`invoke`] / [`invoke_async`]
//! callers.
//!
//! ## Threading
//!
//! Sync [`invoke`] runs on the calling thread. On iOS the bridge
//! dispatches via `NSInvocation`, on Android it attaches the
//! calling thread to the JVM via `AttachCurrentThread` if it
//! isn't already attached — either way, the method body executes
//! on the same thread as the [`invoke`] caller. Methods that
//! touch UIKit / Android Views from a background thread must
//! marshal themselves to the main thread (typical UI-platform
//! contract).
//!
//! [`invoke_async`] uses the bridge's async entry point. v1's
//! implementation dispatches sync; a follow-up wires it through
//! a worker queue with cancel semantics alongside the first
//! actual async-API module.
//!
//! ## Foundation v1
//!
//! [`WhiskerValue`] mirrors the C tagged union but as a safe,
//! Rust-idiomatic enum. Conversions to the platform side allocate
//! through `malloc` (matching the bridge's expected ownership);
//! [`WhiskerValue::from_raw`] copies data OUT of bridge-allocated
//! buffers so the caller can immediately
//! [`whisker_bridge_value_release`](whisker_driver_sys::whisker_bridge_value_release)
//! the underlying allocation.
//!
//! Errors from the bridge (unknown module, missing method,
//! exception thrown) surface as the [`WhiskerValue::Error`]
//! variant carrying a UTF-8 description. The matching
//! `#[whisker::platform_module]` proc macro layer (Phase 7-Φ.E.5)
//! folds those into typed `Result<T, ModuleError>` returns.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::os::raw::c_void;

use whisker_driver_sys as ffi;

/// Tagged-union variant set the Whisker module bridge passes
/// between Rust and the platform side.
///
/// Mirrors the C `WhiskerValueRaw` layout but owns its data —
/// `String`/`Bytes`/`Array`/`Map` are Rust-allocated and dropped
/// on scope exit. Conversion to the C form happens once at the
/// FFI boundary inside [`invoke`].
#[derive(Debug, Clone, PartialEq)]
pub enum WhiskerValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
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
}

/// Failure surface for the `#[whisker::platform_module]` proc-macro-
/// generated proxy methods.
///
/// Wraps the UTF-8 description the bridge returned via
/// [`WhiskerValue::Error`] (unknown module / missing method /
/// platform-side exception / etc.), plus type-mismatch messages
/// the proxy synthesises when the bridge returned an unexpected
/// variant for the declared return type. Implements
/// [`std::error::Error`] so callers can `?`-propagate through
/// `Result` chains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhiskerModuleError(pub String);

impl std::fmt::Display for WhiskerModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WhiskerModuleError {}

// ----- From impls — let callers pass primitives directly ------------------

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

// ----- Sync invoke --------------------------------------------------------

/// Call the registered native module's method, synchronously.
///
/// Returns a [`WhiskerValue::Error`] on dispatch failure (unknown
/// module, missing method, platform-side exception). The bridge's
/// platform-side dispatch + value conversion happen inside this
/// call — see `crates/whisker-driver-sys/bridge/src/
/// whisker_bridge_ios.mm` and `whisker_bridge_android.cc` for the
/// per-platform details.
pub fn invoke(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let module_c = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("module name contained NUL byte".into()),
    };
    let method_c = match CString::new(method) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("method name contained NUL byte".into()),
    };

    // Build a flat array of `WhiskerValueRaw` whose heap-owned
    // allocations stay rooted in the `RawBuilder` until the FFI
    // call returns.
    let mut builder = RawBuilder::default();
    let raw_args: Vec<ffi::WhiskerValueRaw> = args.iter().map(|v| builder.encode(v)).collect();

    let raw_result = unsafe {
        ffi::whisker_bridge_invoke_module(
            module_c.as_ptr(),
            method_c.as_ptr(),
            if raw_args.is_empty() {
                std::ptr::null()
            } else {
                raw_args.as_ptr()
            },
            raw_args.len(),
        )
    };

    let result = unsafe { from_raw(&raw_result) };
    // The bridge owns any heap allocations attached to the
    // returned value — release them now that we've copied the
    // data out into Rust-owned storage.
    unsafe {
        let mut mutable = raw_result;
        ffi::whisker_bridge_value_release(&mut mutable as *mut _);
    }
    drop(builder);
    result
}

/// Async variant. Resolves once the bridge fires the C callback
/// with the result.
///
/// v1 dispatches the work on the bridge's underlying thread (iOS:
/// global dispatch queue; Android: same-thread for now). Cancel
/// semantics + worker-pool dispatch land alongside the first
/// async-API module.
pub async fn invoke_async(name: &str, method: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
    let module_c = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("module name contained NUL byte".into()),
    };
    let method_c = match CString::new(method) {
        Ok(c) => c,
        Err(_) => return WhiskerValue::Error("method name contained NUL byte".into()),
    };

    let mut builder = RawBuilder::default();
    let raw_args: Vec<ffi::WhiskerValueRaw> = args.iter().map(|v| builder.encode(v)).collect();

    let (tx, rx) = futures_channel::oneshot::channel::<WhiskerValue>();
    // Box the sender so we can pass a stable pointer to the C
    // callback. The callback drops the box after firing tx.
    let tx_box: Box<Option<futures_channel::oneshot::Sender<WhiskerValue>>> = Box::new(Some(tx));
    let tx_ptr = Box::into_raw(tx_box) as *mut c_void;

    let ok = unsafe {
        ffi::whisker_bridge_invoke_module_async(
            module_c.as_ptr(),
            method_c.as_ptr(),
            if raw_args.is_empty() {
                std::ptr::null()
            } else {
                raw_args.as_ptr()
            },
            raw_args.len(),
            async_trampoline,
            tx_ptr,
        )
    };
    drop(builder);
    if !ok {
        // Bridge refused to dispatch — recover the sender so the
        // box doesn't leak, then resolve immediately with an
        // error.
        let mut sender_box = unsafe {
            Box::from_raw(tx_ptr as *mut Option<futures_channel::oneshot::Sender<WhiskerValue>>)
        };
        let _ = sender_box.take(); // drop the channel
        return WhiskerValue::Error("bridge refused async dispatch".into());
    }

    rx.await
        .unwrap_or_else(|_| WhiskerValue::Error("async callback never fired".into()))
}

extern "C" fn async_trampoline(user_data: *mut c_void, result: *const ffi::WhiskerValueRaw) {
    if user_data.is_null() || result.is_null() {
        return;
    }
    let mut sender_box = unsafe {
        Box::from_raw(user_data as *mut Option<futures_channel::oneshot::Sender<WhiskerValue>>)
    };
    let value = unsafe { from_raw(&*result) };
    if let Some(sender) = sender_box.take() {
        let _ = sender.send(value);
    }
}

// ----- Raw conversion plumbing -------------------------------------------

/// Owns the heap allocations a [`WhiskerValue`] tree needs when
/// it crosses the FFI boundary. The `WhiskerValueRaw`s the
/// builder produces reference into these allocations; dropping
/// the builder after the FFI call frees everything.
#[derive(Default)]
/// Pinned storage for the heap allocations referenced by a flat
/// `WhiskerValueRaw[]` handed to the C bridge. Keep the builder
/// alive until the FFI call returns; the bridge borrows pointers
/// out of it for the duration of the call.
pub(crate) struct RawBuilder {
    /// `CString`s back the `WhiskerValueRaw::s` pointers. The
    /// FFI pointer points to the CString's internal buffer (a
    /// heap allocation owned by the CString's inner `Vec<u8>`);
    /// the buffer's address doesn't change when the outer
    /// `Vec<CString>` reallocates — only the CString *headers*
    /// (3-word `Vec<u8>` `{ptr, len, cap}`) move, and they don't
    /// hold the FFI-visible bytes.
    strings: Vec<CString>,
    /// Owned `Vec<u8>` for bytes values. Same pointer-stability
    /// rule as `strings`: `Vec::as_ptr` returns the heap buffer's
    /// address, which is stable across moves of the Vec header.
    bytes: Vec<Vec<u8>>,
    /// Owned arrays of nested `WhiskerValueRaw` for the array
    /// variant. Each nested element may itself have heap
    /// allocations owned by `self`.
    arrays: Vec<Vec<ffi::WhiskerValueRaw>>,
    /// Owned arrays of `WhiskerKeyValueRaw` for the map variant.
    maps: Vec<Vec<ffi::WhiskerKeyValueRaw>>,
}

impl RawBuilder {
    pub(crate) fn encode(&mut self, v: &WhiskerValue) -> ffi::WhiskerValueRaw {
        match v {
            WhiskerValue::Null => empty_raw(ffi::WhiskerValueType::Null),
            WhiskerValue::Bool(b) => {
                let mut raw = empty_raw(ffi::WhiskerValueType::Bool);
                raw.v.b = *b;
                raw
            }
            WhiskerValue::Int(i) => {
                let mut raw = empty_raw(ffi::WhiskerValueType::Int);
                raw.v.i = *i;
                raw
            }
            WhiskerValue::Float(f) => {
                let mut raw = empty_raw(ffi::WhiskerValueType::Float);
                raw.v.f = *f;
                raw
            }
            WhiskerValue::String(s) => {
                let c = CString::new(s.as_str()).unwrap_or_default();
                let mut raw = empty_raw(ffi::WhiskerValueType::String);
                raw.v.s = ffi::WhiskerStringRef {
                    ptr: c.as_ptr(),
                    len: s.len(),
                };
                self.strings.push(c);
                raw
            }
            WhiskerValue::Bytes(b) => {
                let owned = b.clone();
                let mut raw = empty_raw(ffi::WhiskerValueType::Bytes);
                raw.v.bytes = ffi::WhiskerBytesRef {
                    ptr: owned.as_ptr(),
                    len: owned.len(),
                };
                self.bytes.push(owned);
                raw
            }
            WhiskerValue::Array(items) => {
                let mut nested: Vec<ffi::WhiskerValueRaw> =
                    items.iter().map(|item| self.encode(item)).collect();
                let mut raw = empty_raw(ffi::WhiskerValueType::Array);
                raw.v.array = ffi::WhiskerValueArray {
                    items: nested.as_mut_ptr(),
                    count: nested.len(),
                };
                self.arrays.push(nested);
                raw
            }
            WhiskerValue::Map(map) => {
                let mut entries: Vec<ffi::WhiskerKeyValueRaw> = map
                    .iter()
                    .map(|(k, v)| {
                        let key_c = CString::new(k.as_str()).unwrap_or_default();
                        let key_ref = ffi::WhiskerStringRef {
                            ptr: key_c.as_ptr(),
                            len: k.len(),
                        };
                        self.strings.push(key_c);
                        let value = self.encode(v);
                        ffi::WhiskerKeyValueRaw {
                            key: key_ref,
                            value,
                        }
                    })
                    .collect();
                let mut raw = empty_raw(ffi::WhiskerValueType::Map);
                raw.v.map = ffi::WhiskerValueMap {
                    entries: entries.as_mut_ptr(),
                    count: entries.len(),
                };
                self.maps.push(entries);
                raw
            }
            WhiskerValue::Error(msg) => {
                let c = CString::new(msg.as_str()).unwrap_or_default();
                let mut raw = empty_raw(ffi::WhiskerValueType::Error);
                raw.v.s = ffi::WhiskerStringRef {
                    ptr: c.as_ptr(),
                    len: msg.len(),
                };
                self.strings.push(c);
                raw
            }
        }
    }
}

fn empty_raw(ty: ffi::WhiskerValueType) -> ffi::WhiskerValueRaw {
    // Zero-init the union — the discriminator is what readers
    // consult to decide which variant is live.
    ffi::WhiskerValueRaw {
        type_: ty as u8,
        _pad: [0; 7],
        v: ffi::WhiskerValueUnion { i: 0 },
    }
}

/// Copy a `WhiskerValueRaw`'s data into a Rust-owned
/// [`WhiskerValue`]. Safe to call on any well-formed raw produced
/// by the bridge; after this returns the caller is free to call
/// `whisker_bridge_value_release` on the raw without invalidating
/// the returned tree.
///
/// # Safety
///
/// `raw.type_` must accurately discriminate the live union member.
/// The `ptr`/`len` pairs in string/bytes variants must be a valid
/// allocation (typically bridge-owned heap) of at least `len`
/// bytes — i.e. only call this on the result of an
/// `invoke_module*` invocation or another bridge-produced
/// `WhiskerValueRaw`.
pub unsafe fn from_raw(raw: &ffi::WhiskerValueRaw) -> WhiskerValue {
    match raw.type_ {
        x if x == ffi::WhiskerValueType::Null as u8 => WhiskerValue::Null,
        x if x == ffi::WhiskerValueType::Bool as u8 => WhiskerValue::Bool(raw.v.b),
        x if x == ffi::WhiskerValueType::Int as u8 => WhiskerValue::Int(raw.v.i),
        x if x == ffi::WhiskerValueType::Float as u8 => WhiskerValue::Float(raw.v.f),
        x if x == ffi::WhiskerValueType::String as u8 => WhiskerValue::String(read_string(raw.v.s)),
        x if x == ffi::WhiskerValueType::Bytes as u8 => {
            WhiskerValue::Bytes(read_bytes(raw.v.bytes))
        }
        x if x == ffi::WhiskerValueType::Array as u8 => {
            let arr = raw.v.array;
            let mut out = Vec::with_capacity(arr.count);
            for i in 0..arr.count {
                let item = &*arr.items.add(i);
                out.push(from_raw(item));
            }
            WhiskerValue::Array(out)
        }
        x if x == ffi::WhiskerValueType::Map as u8 => {
            let m = raw.v.map;
            let mut out = BTreeMap::new();
            for i in 0..m.count {
                let entry = &*m.entries.add(i);
                let key = read_string(entry.key);
                let value = from_raw(&entry.value);
                out.insert(key, value);
            }
            WhiskerValue::Map(out)
        }
        x if x == ffi::WhiskerValueType::Error as u8 => WhiskerValue::Error(read_string(raw.v.s)),
        // Bridge produced an unknown discriminant — surface as
        // an error rather than panicking. Indicates the bridge
        // and the Rust mirror have drifted out of sync.
        other => WhiskerValue::Error(format!(
            "WhiskerValueRaw carries unknown type discriminant {other}"
        )),
    }
}

unsafe fn read_string(r: ffi::WhiskerStringRef) -> String {
    if r.ptr.is_null() || r.len == 0 {
        return String::new();
    }
    // Don't assume NUL-termination — `len` is authoritative.
    let bytes = std::slice::from_raw_parts(r.ptr as *const u8, r.len);
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        // If the bridge produced non-UTF8, fall through to a
        // lossy conversion so we never panic at the FFI seam.
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

unsafe fn read_bytes(r: ffi::WhiskerBytesRef) -> Vec<u8> {
    if r.ptr.is_null() || r.len == 0 {
        return Vec::new();
    }
    std::slice::from_raw_parts(r.ptr, r.len).to_vec()
}

// ----- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Roundtrip every variant through `RawBuilder::encode` →
    /// `from_raw` and verify the original `WhiskerValue` survives.
    /// The bridge isn't called — we exercise just the Rust ↔ raw
    /// conversion path.
    #[test]
    fn roundtrip_all_variants() {
        let cases = vec![
            WhiskerValue::Null,
            WhiskerValue::Bool(true),
            WhiskerValue::Bool(false),
            WhiskerValue::Int(0),
            WhiskerValue::Int(i64::MAX),
            WhiskerValue::Int(i64::MIN),
            WhiskerValue::Float(0.0),
            WhiskerValue::Float(std::f64::consts::PI),
            WhiskerValue::String(String::new()),
            WhiskerValue::String("hello".into()),
            WhiskerValue::String("Whisker 🐱".into()),
            WhiskerValue::Bytes(Vec::new()),
            WhiskerValue::Bytes(vec![0, 1, 2, 255]),
            WhiskerValue::Array(vec![]),
            WhiskerValue::Array(vec![
                WhiskerValue::Int(1),
                WhiskerValue::String("two".into()),
                WhiskerValue::Bool(true),
            ]),
            WhiskerValue::map([("k1", WhiskerValue::Int(1))]),
            WhiskerValue::map([
                (
                    "nested",
                    WhiskerValue::Array(vec![WhiskerValue::Bool(true)]),
                ),
                ("k", WhiskerValue::String("v".into())),
            ]),
            WhiskerValue::Error("boom".into()),
        ];

        for case in cases {
            let mut builder = RawBuilder::default();
            let raw = builder.encode(&case);
            let decoded = unsafe { from_raw(&raw) };
            assert_eq!(case, decoded, "roundtrip mismatch for {case:?}");
            drop(builder);
        }
    }

    /// `From<T>` shortcuts.
    #[test]
    fn from_impls() {
        assert_eq!(WhiskerValue::from(()), WhiskerValue::Null);
        assert_eq!(WhiskerValue::from(true), WhiskerValue::Bool(true));
        assert_eq!(WhiskerValue::from(42_i64), WhiskerValue::Int(42));
        assert_eq!(WhiskerValue::from(42_i32), WhiskerValue::Int(42));
        assert_eq!(WhiskerValue::from(42_u32), WhiskerValue::Int(42));
        assert_eq!(WhiskerValue::from(1.5_f64), WhiskerValue::Float(1.5));
        assert_eq!(WhiskerValue::from("hi"), WhiskerValue::String("hi".into()));
        assert_eq!(
            WhiskerValue::from(vec![1_u8, 2, 3]),
            WhiskerValue::Bytes(vec![1, 2, 3])
        );
        assert_eq!(
            WhiskerValue::from(vec![true, false]),
            WhiskerValue::Array(vec![WhiskerValue::Bool(true), WhiskerValue::Bool(false)])
        );
    }

    /// FFI struct sizes — sanity check that the Rust mirror
    /// agrees with the C definition. WhiskerValueRaw should be
    /// 24 bytes (1 discriminant + 7 pad + 16 union); larger
    /// indicates a layout drift between platforms.
    #[test]
    fn ffi_struct_sizes() {
        assert_eq!(std::mem::size_of::<ffi::WhiskerValueRaw>(), 24);
        assert_eq!(std::mem::size_of::<ffi::WhiskerStringRef>(), 16);
        assert_eq!(std::mem::size_of::<ffi::WhiskerBytesRef>(), 16);
        assert_eq!(std::mem::size_of::<ffi::WhiskerValueArray>(), 16);
        assert_eq!(std::mem::size_of::<ffi::WhiskerValueMap>(), 16);
        // WhiskerKeyValueRaw = 16 (key WhiskerStringRef) + 24
        // (value WhiskerValueRaw) = 40.
        assert_eq!(std::mem::size_of::<ffi::WhiskerKeyValueRaw>(), 40);
    }

    /// `as_error` shortcut.
    #[test]
    fn as_error() {
        let err = WhiskerValue::Error("boom".into());
        assert_eq!(err.as_error(), Some("boom"));
        assert_eq!(WhiskerValue::Null.as_error(), None);
        assert_eq!(WhiskerValue::Int(5).as_error(), None);
    }
}
