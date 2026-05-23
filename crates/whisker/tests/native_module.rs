//! `#[whisker::native_module]` integration tests.
//!
//! We can't exercise the bridge end-to-end here — the bridge
//! requires a registered platform-side module (Swift class on
//! iOS, Kotlin class on Android). Host tests instead verify:
//!
//! 1. The macro expands cleanly for the documented input shapes
//!    (compile-time only; nothing run-time-exercised).
//! 2. Calls into the macro-emitted methods on host return a
//!    `WhiskerValue::Error("module not registered")` value
//!    without crashing — the bridge's
//!    `whisker_bridge_invoke_module` stub responds that way when
//!    no platform module is registered for the requested name.
//!
//! Real end-to-end verification lives in Phase 7-Φ.F.s6
//! (whisker-local-store + hello-world integration on iOS
//! simulator + Android emulator).

use whisker::native_module::WhiskerValue;

// ----- Sync proxy ---------------------------------------------------------

#[whisker::native_module(name = "WhiskerStorage")]
pub trait WhiskerStorageSys {
    fn save(args: Vec<WhiskerValue>) -> WhiskerValue;
    fn load(args: Vec<WhiskerValue>) -> WhiskerValue;
    fn clear(args: Vec<WhiskerValue>) -> WhiskerValue;
}

#[test]
fn sync_proxy_unregistered_module_returns_err_value() {
    // No platform module registered → bridge returns an Error
    // WhiskerValue. The proxy returns it verbatim — type-safe
    // unwrap is the author wrapper's job, not the macro's.
    let result = WhiskerStorageSys::save(vec![
        WhiskerValue::String("k".into()),
        WhiskerValue::String("v".into()),
    ]);
    assert!(matches!(result, WhiskerValue::Error(_)));
}

#[test]
fn sync_proxy_single_arg_passthrough() {
    let result = WhiskerStorageSys::load(vec![WhiskerValue::String("k".into())]);
    assert!(matches!(result, WhiskerValue::Error(_)));
}

#[test]
fn sync_proxy_empty_arg_vec() {
    let result = WhiskerStorageSys::clear(vec![]);
    assert!(matches!(result, WhiskerValue::Error(_)));
}

// ----- Async proxy --------------------------------------------------------

#[whisker::native_module(name = "WhiskerHttp")]
pub trait WhiskerHttpSys {
    async fn fetch(args: Vec<WhiskerValue>) -> WhiskerValue;
}

#[test]
fn async_proxy_compiles() {
    // We can't `await` here without an executor — just confirm
    // the macro emits a callable signature. The future itself
    // can be constructed safely (constructing doesn't dispatch).
    let _fut = WhiskerHttpSys::fetch(vec![WhiskerValue::String("https://example.com".into())]);
}

// ----- Custom module name -------------------------------------------------

#[whisker::native_module(name = "OverriddenName")]
pub trait MyLocalTraitSys {
    fn ping(args: Vec<WhiskerValue>) -> WhiskerValue;
}

#[test]
fn custom_module_name_compiles() {
    let _ = MyLocalTraitSys::ping(vec![]);
}

// ----- Default name (= trait name) ----------------------------------------

#[whisker::native_module]
pub trait UnnamedModule {
    fn echo(args: Vec<WhiskerValue>) -> WhiskerValue;
}

#[test]
fn default_module_name_compiles() {
    let _ = UnnamedModule::echo(vec![WhiskerValue::String("x".into())]);
}
