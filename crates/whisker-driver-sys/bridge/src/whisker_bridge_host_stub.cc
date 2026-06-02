// Host-build stub for the C ABI surface.
//
// Compiled by `build.rs` ONLY when the cargo target isn't iOS or
// Android — `cargo test` / `cargo build` on a developer's macOS /
// Linux box lands here so the Rust crates that consume the bridge
// (whisker-driver, whisker, the proc-macro-generated proxies) link
// cleanly without pulling in the Lynx C API symbols
// (lynx_shell_*, lynx_element_*, lynx_create_fiber_element*) that
// `whisker_bridge_common.cc` calls.
//
// Phase 7-Φ.F: the host stub now implements the pure-C dispatch
// table for native modules so host tests of the
// `#[whisker::native_module]` proxies can exercise the
// register-then-invoke flow without a real Swift / Kotlin
// implementation. Without a registered dispatch fn,
// `whisker_bridge_invoke_module` returns a `WHISKER_VALUE_ERROR`
// — same shape as the iOS / Android paths report when nothing's
// wired up — which the proxy / wrapper layer surfaces as
// `WhiskerValue::Error(_)` to the caller.

#include "whisker_bridge.h"

#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <unordered_map>

namespace {

std::mutex& ModuleRegistryMutex() {
    static std::mutex m;
    return m;
}
std::unordered_map<std::string, WhiskerModuleDispatchFn>& ModuleRegistry() {
    static std::unordered_map<std::string, WhiskerModuleDispatchFn> m;
    return m;
}

WhiskerValueRaw MakeHostStubError(const char* message) {
    WhiskerValueRaw v;
    std::memset(&v, 0, sizeof(v));
    v.type = WHISKER_VALUE_ERROR;
    if (message != nullptr) {
        size_t len = std::strlen(message);
        char* buf = static_cast<char*>(std::malloc(len + 1));
        std::memcpy(buf, message, len + 1);
        v.v.s.ptr = buf;
        v.v.s.len = len;
    }
    return v;
}

}  // namespace

extern "C" void whisker_bridge_register_module_dispatch(
    const char* module_name,
    WhiskerModuleDispatchFn dispatch) {
    if (module_name == nullptr) return;
    std::lock_guard<std::mutex> g(ModuleRegistryMutex());
    if (dispatch == nullptr) {
        ModuleRegistry().erase(module_name);
    } else {
        ModuleRegistry()[module_name] = dispatch;
    }
}

extern "C" WhiskerValueRaw whisker_bridge_invoke_module(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count) {
    if (module_name == nullptr || method_name == nullptr) {
        return MakeHostStubError("module/method name is NULL");
    }
    WhiskerModuleDispatchFn fn = nullptr;
    {
        std::lock_guard<std::mutex> g(ModuleRegistryMutex());
        auto it = ModuleRegistry().find(module_name);
        if (it != ModuleRegistry().end()) {
            fn = it->second;
        }
    }
    if (fn == nullptr) {
        return MakeHostStubError(
            "whisker_bridge_invoke_module: host build has no platform "
            "module registered for this name — link against the iOS / "
            "Android bridge for real module invocation");
    }
    return fn(method_name, args, arg_count);
}

extern "C" WhiskerValueRaw whisker_bridge_invoke_element_method(
    WhiskerElement* /*element*/,
    const char* /*method_name*/,
    const WhiskerValueRaw* /*args*/,
    size_t /*arg_count*/) {
    // Same shape as the production stub in `whisker_bridge_common.cc`
    // — host builds (cargo test) don't have Lynx, so the call is a
    // pure error path. Phase 7-Φ.H.2.5.
    return MakeHostStubError(
        "whisker_bridge_invoke_element_method: host build has no Lynx — "
        "link against the iOS / Android bridge for real element-method "
        "dispatch (which itself is currently a stub pending Phase 7-Φ.H.2.7)");
}

extern "C" WhiskerValueRaw whisker_bridge_invoke_element_method_with_params(
    WhiskerElement* /*element*/,
    const char* /*method_name*/,
    const WhiskerValueRaw* /*params*/) {
    // Host builds (cargo test) have no Lynx — the params-map dispatch is
    // a pure error path, same shape as the scalar-arg stub above.
    return MakeHostStubError(
        "whisker_bridge_invoke_element_method_with_params: host build has no "
        "Lynx — link against the iOS / Android bridge for real built-in "
        "UI-method dispatch");
}

extern "C" WhiskerValueRaw whisker_bridge_element_animate(
    WhiskerElement* /*element*/,
    int32_t /*operation*/,
    const char* /*animation_name*/,
    const WhiskerValueRaw* /*keyframes*/,
    const WhiskerValueRaw* /*options*/) {
    // Host builds have no Lynx animator — same error-path shape as the
    // other element-method stubs. Real dispatch happens on iOS /
    // Android via `lynx_element_animate`.
    return MakeHostStubError(
        "whisker_bridge_element_animate: host build has no Lynx — link "
        "against the iOS / Android bridge for real animation dispatch");
}

extern "C" bool whisker_bridge_invoke_module_async(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (callback == nullptr) return false;
    WhiskerValueRaw result = whisker_bridge_invoke_module(
        module_name, method_name, args, arg_count);
    callback(user_data, &result);
    whisker_bridge_value_release(&result);
    return true;
}

extern "C" bool whisker_bridge_invoke_element_method_async_with_params(
    WhiskerElement* /*element*/,
    const char* /*method_name*/,
    const WhiskerValueRaw* /*params*/,
    WhiskerModuleCallback callback,
    void* user_data) {
    // Host build has no Lynx — resolve to an error so the Rust future
    // completes (mirrors the args-array async stub below).
    if (callback == nullptr) return false;
    WhiskerValueRaw err = MakeHostStubError(
        "whisker_bridge_invoke_element_method_async_with_params: host build "
        "has no Lynx");
    callback(user_data, &err);
    whisker_bridge_value_release(&err);
    return true;
}

extern "C" bool whisker_bridge_invoke_element_method_async(
    WhiskerElement* /*element*/,
    const char* /*method_name*/,
    const WhiskerValueRaw* /*args*/,
    size_t /*arg_count*/,
    WhiskerModuleCallback callback,
    void* user_data) {
    // Host build has no Lynx — resolve to an error so the Rust future
    // completes rather than hanging.
    if (callback == nullptr) return false;
    WhiskerValueRaw result = MakeHostStubError(
        "whisker_bridge_invoke_element_method_async: host build has no Lynx");
    callback(user_data, &result);
    whisker_bridge_value_release(&result);
    return false;
}

extern "C" void whisker_bridge_value_release(WhiskerValueRaw* value) {
    if (value == nullptr) return;
    switch (value->type) {
        case WHISKER_VALUE_STRING:
        case WHISKER_VALUE_ERROR:
            std::free(const_cast<char*>(value->v.s.ptr));
            value->v.s.ptr = nullptr;
            value->v.s.len = 0;
            break;
        case WHISKER_VALUE_BYTES:
            std::free(const_cast<uint8_t*>(value->v.bytes.ptr));
            value->v.bytes.ptr = nullptr;
            value->v.bytes.len = 0;
            break;
        case WHISKER_VALUE_ARRAY:
            for (size_t i = 0; i < value->v.array.count; i++) {
                whisker_bridge_value_release(&value->v.array.items[i]);
            }
            std::free(value->v.array.items);
            value->v.array.items = nullptr;
            value->v.array.count = 0;
            break;
        case WHISKER_VALUE_MAP:
            for (size_t i = 0; i < value->v.map.count; i++) {
                std::free(const_cast<char*>(value->v.map.entries[i].key.ptr));
                whisker_bridge_value_release(&value->v.map.entries[i].value);
            }
            std::free(value->v.map.entries);
            value->v.map.entries = nullptr;
            value->v.map.count = 0;
            break;
        default:
            break;
    }
    value->type = WHISKER_VALUE_NULL;
}

extern "C" void whisker_bridge_log_hello(void) {
    // Empty stub — the real impl lives in the iOS / Android bridge
    // files; host code that calls this gets a no-op.
}

// ---------- whisker-module event system (Phase L-2c) ----------------------
//
// Host builds have no native module sending events, so the event-
// listener registry collapses to no-ops: `add_event_listener` returns
// a fake-positive id (so the Rust wrapper's "subscription succeeded"
// check passes), and the rest are silent no-ops. Tests that rely on
// real Rust ↔ native event flow have to run on iOS / Android.

extern "C" int32_t whisker_bridge_module_add_event_listener(
    const char* /*module_name*/,
    const char* /*event_name*/,
    WhiskerModuleEventCallback /*callback*/,
    void* /*user_data*/) {
    // Return a non-zero id so the Rust `ModuleSubscription::error()`
    // path doesn't flag the registration as failed in host tests.
    return 1;
}

extern "C" void whisker_bridge_module_remove_event_listener(
    int32_t /*listener_id*/) {
    // No-op — there's nothing registered in the host stub.
}

extern "C" void whisker_bridge_module_send_event(
    const char* /*module_name*/,
    const char* /*event_name*/,
    const WhiskerValueRaw* /*payload*/) {
    // No-op — no native module fires events in host tests.
}

extern "C" void whisker_bridge_module_register_observer_hooks(
    const char* /*module_name*/,
    WhiskerModuleObserverHook /*started*/,
    WhiskerModuleObserverHook /*stopped*/) {
    // No-op — no observer-driven sources in host tests.
}

extern "C" void whisker_bridge_log_info(
    const char* /*tag*/, const char* /*msg*/) {
    // No-op — real impl writes to logcat (Android) / os_log (iOS).
}
