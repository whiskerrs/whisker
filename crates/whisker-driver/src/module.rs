//! Whisker platform module invocation — Rust-side entry point for
//! the C bridge `whisker_bridge_invoke_module*` family
//! (`whisker-driver-sys`).
//!
//! The platform-side dispatch (NSInvocation on iOS, JNI cached
//! `jmethodID` on Android) lives in
//! `crates/whisker-driver-sys/bridge/src/`; this module wraps the
//! raw `WhiskerValueRaw` C-mirror in a typed [`WhiskerValue`]
//! Rust enum and provides ergonomic [`invoke`] / [`invoke_async`]
//! callers, plus the user-facing [`PlatformModule`] handle.
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
//! ## Value model
//!
//! [`WhiskerValue`] mirrors the C tagged union but as a safe,
//! Rust-idiomatic enum. Conversions to the platform side allocate
//! through `malloc` (matching the bridge's expected ownership);
//! [`from_raw`] copies data OUT of bridge-allocated buffers so the
//! caller can immediately
//! [`whisker_bridge_value_release`](whisker_driver_sys::whisker_bridge_value_release)
//! the underlying allocation.
//!
//! Errors from the bridge (unknown module, missing method,
//! exception thrown) surface as the [`WhiskerValue::Error`] variant
//! carrying a UTF-8 description. The matching
//! `#[whisker::platform_module]` proc macro layer folds those into
//! typed `Result<T, ModuleError>` returns.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::os::raw::c_void;

use whisker_driver_sys as ffi;

// `WhiskerValue` / `WhiskerModuleError` moved down to
// `whisker-runtime` (the value model the renderer's event-listener
// trait must name; `whisker-driver` depends on the runtime, not the
// reverse). Re-exported here so existing
// `whisker_driver::module::WhiskerValue` paths keep resolving — the
// FFI marshalling (`RawBuilder` / `from_raw` / `invoke`) stays in
// this module.
pub use whisker_runtime::value::{WhiskerModuleError, WhiskerValue};

// ----- Sync invoke --------------------------------------------------------

/// Call the registered platform module's method, synchronously.
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

/// A handle to a native Whisker (function-only) module, identified by
/// its **fully-qualified** name (`<crate>:<Name>`).
///
/// Construct it with the [`whisker::module!`](../macro.module.html)
/// macro — which prepends the calling crate's name so two crates can
/// ship same-named modules without colliding — or with
/// [`PlatformModule::named`] when you already hold the qualified name.
///
/// Mirrors Expo's `requireNativeModule(name)`: a lightweight,
/// name-keyed reference. No registry lookup happens here; an
/// unregistered module surfaces as a [`WhiskerValue::Error`] at
/// [`invoke`](Self::invoke) time.
///
/// Module authors layer a typed wrapper on top (build the args,
/// dispatch through `invoke`, lift the returned `WhiskerValue` into a
/// typed `Result`). The raw `WhiskerValue` surface stays so a
/// conversion mistake is loggable rather than silently lost.
///
/// ```ignore
/// let module = whisker::module!("Battery");
/// match module.invoke("getLevel", vec![]) {
///     WhiskerValue::Float(level) => println!("battery: {level:.0}%"),
///     WhiskerValue::Error(msg) => eprintln!("battery dispatch failed: {msg}"),
///     other => eprintln!("unexpected reply: {other:?}"),
/// }
/// ```
#[derive(Debug, Clone)]
pub struct PlatformModule {
    name: String,
}

impl PlatformModule {
    /// Reference the module with the given **fully-qualified** name
    /// (`<crate>:<Name>`). Prefer the `whisker::module!("Name")` macro,
    /// which supplies the `<crate>:` prefix automatically from the
    /// calling crate's `CARGO_PKG_NAME`.
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// The fully-qualified module name this handle dispatches to.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Dispatch `function` with `args` to the native module. Returns
    /// the raw [`WhiskerValue`] (with [`WhiskerValue::Error`] on
    /// dispatch failure); the typed wrapper converts it.
    pub fn invoke(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke(&self.name, function, args)
    }

    /// Async variant of [`invoke`](Self::invoke).
    pub async fn invoke_async(&self, function: &str, args: Vec<WhiskerValue>) -> WhiskerValue {
        invoke_async(&self.name, function, args).await
    }

    /// Subscribe to `event` on this module. The returned
    /// [`ModuleSubscription`] removes the listener on drop, so the
    /// caller controls listener lifetime by simply holding (or
    /// dropping) the value.
    ///
    /// Modelled on Expo's `addListener(event, fn)` — the native side
    /// pumps events through `whisker_bridge_module_send_event`, the
    /// bridge fans them out to every registered Rust callback.
    /// `OnStartObserving` / `OnStopObserving` hooks (registered
    /// per-module on the platform side) fire on the 0↔1 listener
    /// transition so the module can lazily attach / detach its
    /// underlying source.
    ///
    /// The closure runs on whichever thread the bridge dispatches on
    /// (typically the platform main thread). `Send + Sync` is
    /// required because the bridge stores the pointer and the
    /// dispatch thread may differ from the subscribing thread.
    pub fn on_event<F>(&self, event: &str, callback: F) -> ModuleSubscription
    where
        F: Fn(WhiskerValue) + Send + Sync + 'static,
    {
        let module_c = match CString::new(self.name.as_str()) {
            Ok(c) => c,
            Err(_) => return ModuleSubscription::failed("module name contained NUL byte"),
        };
        let event_c = match CString::new(event) {
            Ok(c) => c,
            Err(_) => return ModuleSubscription::failed("event name contained NUL byte"),
        };
        // `Box<EventCallback>` gives a stable thin pointer we can
        // stuff into `user_data`. The inner box carries the fat
        // trait-object pointer; the outer box is just for the address.
        let callback: EventCallback = Box::new(callback);
        let user_data = Box::into_raw(Box::new(callback)) as *mut c_void;
        let id = unsafe {
            ffi::whisker_bridge_module_add_event_listener(
                module_c.as_ptr(),
                event_c.as_ptr(),
                event_trampoline,
                user_data,
            )
        };
        if id <= 0 {
            // Bridge refused — recover the box so the closure drops.
            unsafe {
                let _ = Box::from_raw(user_data as *mut EventCallback);
            }
            return ModuleSubscription::failed("bridge refused event listener registration");
        }
        ModuleSubscription {
            id,
            user_data,
            error: None,
        }
    }
}

// ----- Event subscription -------------------------------------------------

/// Heap-allocated Rust callback the C trampoline dispatches into.
/// `Send + Sync` because the bridge may fire from any thread.
type EventCallback = Box<dyn Fn(WhiskerValue) + Send + Sync>;

/// C trampoline registered with the bridge. Recovers the
/// `EventCallback` from `user_data` (by reference — ownership stays
/// with [`ModuleSubscription`]) and invokes it with the decoded
/// payload.
extern "C" fn event_trampoline(user_data: *mut c_void, payload: *const ffi::WhiskerValueRaw) {
    if user_data.is_null() {
        return;
    }
    let cb = unsafe { &*(user_data as *const EventCallback) };
    let value = if payload.is_null() {
        WhiskerValue::Null
    } else {
        unsafe { from_raw(&*payload) }
    };
    cb(value);
}

/// RAII handle for an [`PlatformModule::on_event`] subscription.
/// Dropping the handle removes the listener via
/// [`whisker_bridge_module_remove_event_listener`](whisker_driver_sys::whisker_bridge_module_remove_event_listener)
/// and frees the boxed Rust closure.
///
/// `error` carries a non-fatal registration failure so the caller can
/// inspect it without unwrapping a `Result`. A failed subscription is
/// inert — drop does nothing — so it's safe to leak.
pub struct ModuleSubscription {
    id: i32,
    user_data: *mut c_void,
    error: Option<String>,
}

impl ModuleSubscription {
    fn failed(msg: &str) -> Self {
        Self {
            id: 0,
            user_data: std::ptr::null_mut(),
            error: Some(msg.into()),
        }
    }

    /// `Some(_)` if registration failed; `None` for a live
    /// subscription. The error string is suitable for logging.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Bridge-assigned listener id (positive for live subscriptions,
    /// `0` for a failed one). Exposed primarily for tests / tracing.
    pub fn id(&self) -> i32 {
        self.id
    }
}

// The `user_data` pointer references a `Box<EventCallback>` whose
// inner closure is `Send + Sync`. The wrapper itself never reads the
// pointer except to free it on drop, so it's safe to move across
// threads.
unsafe impl Send for ModuleSubscription {}
unsafe impl Sync for ModuleSubscription {}

impl Drop for ModuleSubscription {
    fn drop(&mut self) {
        if self.id <= 0 || self.user_data.is_null() {
            return;
        }
        unsafe {
            ffi::whisker_bridge_module_remove_event_listener(self.id);
            // Reclaim and drop the boxed closure now that the bridge
            // can no longer call into it.
            let _ = Box::from_raw(self.user_data as *mut EventCallback);
        }
    }
}

/// Async variant of [`invoke`]. Resolves once the bridge fires the C
/// callback with the result.
///
/// Dispatch happens on the bridge's underlying thread (iOS global
/// dispatch queue; Android same-thread). No cancel semantics today —
/// dropping the future leaks the oneshot until the callback fires.
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

/// C callback for the async invoke paths (`invoke_async` /
/// `invoke_element_method_async`). Reconstructs the boxed oneshot
/// sender from `user_data`, decodes the result, and resolves the
/// channel. The bridge guarantees exactly one call per dispatch (sync
/// on failure, async on success), so the box is consumed here and
/// never recovered by the caller.
pub(crate) extern "C" fn async_trampoline(
    user_data: *mut c_void,
    result: *const ffi::WhiskerValueRaw,
) {
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

/// Pinned storage for the heap allocations referenced by a flat
/// `WhiskerValueRaw[]` handed to the C bridge.
///
/// The `WhiskerValueRaw`s the builder produces hold raw pointers into
/// these allocations; keep the builder alive until the FFI call
/// returns, then dropping it frees everything in one shot.
#[derive(Default)]
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
    /// Encode a Rust `&str` into an owned NUL-terminated C string and
    /// return a `WhiskerStringRef` pointing at it. The returned `len`
    /// is derived from the actual buffer, never from the source `&str`.
    ///
    /// Rust `String`s may contain interior NUL bytes; `CString` cannot.
    /// `CString::new` therefore fails on such input and we fall back to
    /// an empty C string. The advertised `len` MUST match that buffer —
    /// the previous code set `len = s.len()` (the *original* length)
    /// while the buffer was the empty fallback, so the C side read past
    /// the 1-byte allocation (OOB read). Deriving `len` from
    /// `c.as_bytes()` keeps the two in lockstep: an interior NUL now
    /// degrades to an empty string instead of an out-of-bounds read.
    fn push_str_ref(&mut self, s: &str) -> ffi::WhiskerStringRef {
        let c = CString::new(s).unwrap_or_default();
        let r = ffi::WhiskerStringRef {
            ptr: c.as_ptr(),
            len: c.as_bytes().len(),
        };
        self.strings.push(c);
        r
    }

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
                let s_ref = self.push_str_ref(s.as_str());
                let mut raw = empty_raw(ffi::WhiskerValueType::String);
                raw.v.s = s_ref;
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
                        let key_ref = self.push_str_ref(k.as_str());
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
                let s_ref = self.push_str_ref(msg.as_str());
                let mut raw = empty_raw(ffi::WhiskerValueType::Error);
                raw.v.s = s_ref;
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

    /// A string with an interior NUL byte must never advertise a `len`
    /// longer than the buffer it points at. `CString` can't hold an
    /// interior NUL, so the encoder falls back to an empty buffer — and
    /// the advertised `len` has to follow, or the C side reads OOB. We
    /// assert the encoded `len` matches the actual NUL-terminated buffer
    /// for both the `String` and `Error` (and map-key) paths.
    #[test]
    fn interior_nul_len_matches_buffer() {
        // SAFETY: we only read the `len` field and compare against the
        // owned buffer the builder keeps alive; no dereference past it.
        let assert_consistent = |value: &WhiskerValue| {
            let mut builder = RawBuilder::default();
            let raw = builder.encode(value);
            // The owned CString backing this ref is the last one pushed.
            let buf = builder.strings.last().expect("string was pushed");
            let advertised = unsafe { raw.v.s.len };
            assert_eq!(
                advertised,
                buf.as_bytes().len(),
                "advertised len must match the owned buffer for {value:?}"
            );
            // Interior NUL can't survive a CString, so it degrades to
            // empty rather than reading out of bounds.
            assert_eq!(advertised, 0, "interior-NUL string degrades to empty");
        };
        assert_consistent(&WhiskerValue::String("a\0b".into()));
        assert_consistent(&WhiskerValue::Error("x\0y".into()));

        // Map key with an interior NUL: encode must not panic and the
        // key ref len must match its buffer.
        let mut builder = RawBuilder::default();
        let raw = builder.encode(&WhiskerValue::map([("k\0ey", WhiskerValue::Int(1))]));
        let key_buf = builder.strings.last().expect("key string was pushed");
        let key_len = unsafe { (*raw.v.map.entries).key.len };
        assert_eq!(key_len, key_buf.as_bytes().len());
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
