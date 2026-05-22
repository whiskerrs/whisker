// Host-build stub for the C ABI surface.
//
// Compiled by `build.rs` ONLY when the cargo target isn't iOS or
// Android — `cargo test` / `cargo build` on a developer's macOS /
// Linux box lands here so the Rust crates that consume the bridge
// (whisker-driver, whisker, the proc-macro-generated proxies) link
// cleanly. The stub returns a `WHISKER_VALUE_ERROR` for every
// invoke; real platform dispatch lives in `whisker_bridge_ios.mm`
// (NSInvocation) and `whisker_bridge_android.cc` (JNI).
//
// Symbols mirror the contracts in `whisker_bridge.h`; ownership
// rules match the platform impls (the returned Error variant
// carries a heap-malloc'd message that `value_release` frees).
//
// Phase 7-Φ.E.5 added this file so host unit tests for the
// `#[whisker::native_module]` proc macro (under
// `crates/whisker/tests/native_module.rs`) can verify the
// generated proxies dispatch to the bridge and gracefully wrap
// the resulting error in `WhiskerModuleError`.

#include "whisker_bridge.h"

#include <cstdlib>
#include <cstring>

namespace {

WhiskerValue MakeHostStubError(const char* message) {
    WhiskerValue v;
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

extern "C" WhiskerValue whisker_bridge_invoke_module(
    const char* /*module_name*/,
    const char* /*method_name*/,
    const WhiskerValue* /*args*/,
    size_t /*arg_count*/) {
    return MakeHostStubError(
        "whisker_bridge_invoke_module: host build has no platform "
        "dispatch — link against the iOS / Android bridge for real "
        "module invocation");
}

extern "C" bool whisker_bridge_invoke_module_async(
    const char* /*module_name*/,
    const char* /*method_name*/,
    const WhiskerValue* /*args*/,
    size_t /*arg_count*/,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (callback == nullptr) return false;
    WhiskerValue err = MakeHostStubError(
        "whisker_bridge_invoke_module_async: host build has no "
        "platform dispatch");
    callback(user_data, &err);
    std::free(const_cast<char*>(err.v.s.ptr));
    return true;
}

extern "C" void whisker_bridge_value_release(WhiskerValue* value) {
    if (value == nullptr) return;
    if (value->type == WHISKER_VALUE_STRING || value->type == WHISKER_VALUE_ERROR) {
        std::free(const_cast<char*>(value->v.s.ptr));
        value->v.s.ptr = nullptr;
        value->v.s.len = 0;
    } else if (value->type == WHISKER_VALUE_BYTES) {
        std::free(const_cast<uint8_t*>(value->v.bytes.ptr));
        value->v.bytes.ptr = nullptr;
        value->v.bytes.len = 0;
    }
    // Array / Map host stubs never produce nested allocations
    // — only Error and Bytes need freeing.
    value->type = WHISKER_VALUE_NULL;
}
