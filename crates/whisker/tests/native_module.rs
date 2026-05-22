//! `#[whisker::native_module]` integration tests.
//!
//! We can't exercise the bridge end-to-end here — the bridge
//! requires a registered platform-side module (Obj-C class on
//! iOS, Kotlin class on Android). Host tests instead verify:
//!
//! 1. The macro expands cleanly for the documented input shapes
//!    (compile-time only; nothing run-time-exercised).
//! 2. Calls into the macro-emitted methods on host return
//!    `Err(WhiskerModuleError)` rather than crashing — the
//!    bridge's `whisker_bridge_invoke_module` stub responds
//!    with `WhiskerValue::Error("module not registered")` when
//!    no platform module class is in the registry.
//!
//! Real end-to-end verification lives in Phase 7-Φ.E.8
//! (whisker-storage-module + hello-world integration on iOS
//! simulator + Android emulator).

use whisker::native_module::WhiskerModuleError;

// ----- Sync proxy ---------------------------------------------------------

#[whisker::native_module(name = "WhiskerStorage")]
pub trait WhiskerStorage {
    fn save(key: String, value: String) -> bool;
    fn load(key: String) -> Option<String>;
    fn clear(key: String) -> ();
}

#[test]
fn sync_proxy_unregistered_module_returns_err() {
    // No platform module registered → bridge returns an Error
    // WhiskerValue. The proxy lifts it into Err.
    let result = WhiskerStorage::save("k".into(), "v".into());
    assert!(matches!(result, Err(WhiskerModuleError(_))));
}

#[test]
fn sync_proxy_option_return_unregistered() {
    let result = WhiskerStorage::load("k".into());
    assert!(matches!(result, Err(WhiskerModuleError(_))));
}

#[test]
fn sync_proxy_unit_return_unregistered() {
    let result = WhiskerStorage::clear("k".into());
    assert!(matches!(result, Err(WhiskerModuleError(_))));
}

// ----- Async proxy --------------------------------------------------------

#[whisker::native_module(name = "WhiskerHttp")]
pub trait WhiskerHttp {
    async fn fetch(url: String) -> Vec<u8>;
}

#[test]
fn async_proxy_compiles() {
    // We can't `await` here without an executor — just confirm
    // the macro emits a callable signature. The future itself
    // can be constructed safely (constructing doesn't dispatch).
    let _fut = WhiskerHttp::fetch("https://example.com".into());
}

// ----- Custom module name -------------------------------------------------

#[whisker::native_module(name = "OverriddenName")]
pub trait MyLocalTrait {
    fn ping() -> bool;
}

#[test]
fn custom_module_name_compiles() {
    let _ = MyLocalTrait::ping();
}

// ----- Default name (= trait name) ----------------------------------------

#[whisker::native_module]
pub trait UnnamedModule {
    fn echo(value: String) -> String;
}

#[test]
fn default_module_name_compiles() {
    let _ = UnnamedModule::echo("x".into());
}
