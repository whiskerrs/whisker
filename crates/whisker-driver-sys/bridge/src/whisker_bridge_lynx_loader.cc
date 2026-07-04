// whisker_bridge_lynx_loader.cc
//
// dlopen-based loader for Lynx's C ABI. Implements
// `whisker_bridge_load_lynx()` and `whisker_lynx_capi()` from
// `lynx_capi.h`.
//
// Why this exists: pre-Step-6, the bridge's `.o` files carried
// link-time UND refs to every `lynx_*` symbol it called, and the
// `whisker-driver-sys` build script emitted `-framework Lynx` (iOS) /
// `-llynx` (Android) so the user-crate dylib's link step could resolve
// them. That forced the user to run `whisker build` once before any
// `cargo build` would succeed (to fetch + stage the Lynx artifact
// tree). Step 6 cuts that requirement: the bridge calls Lynx through
// a function pointer table populated at runtime, so `cargo build`
// only needs the bridge's own sources + system headers — no Lynx.
//
// Runtime guarantee: by the time this loader runs (called from
// `whisker_bridge_internal_engine_create`, itself called from
// `whisker_bridge_engine_attach` after the platform has constructed
// a LynxView), Lynx is already mapped into the process address space
// — on Android by Kotlin's `System.loadLibrary("lynx")` chain, on iOS
// by dyld auto-loading `Lynx.framework` via the SwiftPM-injected
// LC_LOAD_DYLIB entry in the host app. dlopen here returns a handle
// to the already-loaded image; it does NOT trigger a fresh load.

#include "lynx_capi.h"

#include <atomic>
#include <cstdint>
#include <dlfcn.h>
#include <mutex>

#if defined(__ANDROID__)
#include <android/log.h>
#define WHISKER_LOG_TAG "whisker-bridge-loader"
#define WHISKER_LOADER_LOGE(...) \
    __android_log_print(ANDROID_LOG_ERROR, WHISKER_LOG_TAG, __VA_ARGS__)
#elif defined(__APPLE__)
#include <syslog.h>
#define WHISKER_LOADER_LOGE(...) syslog(LOG_ERR, "[whisker-bridge-loader] " __VA_ARGS__)
#else
#include <cstdio>
#define WHISKER_LOADER_LOGE(...)                                \
    do {                                                        \
        fprintf(stderr, "[whisker-bridge-loader] " __VA_ARGS__); \
        fprintf(stderr, "\n");                                  \
    } while (0)
#endif

namespace {

WhiskerLynxCapi g_capi{};
std::once_flag g_load_once;
std::atomic<int> g_load_result{INT32_MIN};  // sentinel = not yet run

// Pick the dlopen target. On both supported platforms Lynx is already
// in memory (see file header), so passing the SONAME / framework name
// just returns the existing handle without re-mapping anything. We
// canNOT use `dlsym(RTLD_DEFAULT, ...)` on Android: the bridge `.so`
// no longer carries `liblynx.so` in its DT_NEEDED list (Step 6 is
// what cut that), so RTLD_DEFAULT — which only searches the caller's
// executable + its DT_NEEDED dependencies + RTLD_GLOBAL-opened libs —
// doesn't see Lynx's exports. dlopen + dlsym(handle, ...) bypasses
// that scoping by looking up against the handle directly.
const char* LynxSoname() {
#if defined(__ANDROID__)
    return "liblynx.so";
#elif defined(__APPLE__)
    // SwiftPM auto-embeds `Lynx.framework` under `<App>.app/Frameworks/`
    // and the host app's LD_RUNPATH_SEARCH_PATHS includes
    // `@executable_path/Frameworks`, so `@rpath/Lynx.framework/Lynx`
    // resolves via dyld.
    return "@rpath/Lynx.framework/Lynx";
#else
    return nullptr;
#endif
}

// Resolve a single symbol via `dlsym(handle, name)` or set
// `*ok = false`. `handle` is the value returned by the earlier
// `dlopen` of Lynx.
//
// Subtle bug worth a paragraph: the dlerror() queue is process-wide.
// The first call to dlerror() drains the queue; subsequent calls
// return NULL until something else sets it. A naive
// `dlerror() ? dlerror() : "..."` calls it once to check, then AGAIN
// to use — the printed string is always `(null)` even when there
// was a real error message. We capture dlerror() exactly once and
// re-use the captured pointer.
template <typename Fn>
void BindSymbol(void* handle, const char* name, Fn* out, bool* ok) {
    (void)dlerror();  // drain any stale entry left by a previous caller
    void* sym = dlsym(handle, name);
    if (sym == nullptr) {
        const char* err = dlerror();
        WHISKER_LOADER_LOGE("dlsym(%s) failed: %s",
                            name,
                            err != nullptr ? err : "(no dlerror message)");
        *ok = false;
        return;
    }
    *out = reinterpret_cast<Fn>(sym);
}

int DoLoad() {
    const char* soname = LynxSoname();
    if (soname == nullptr) {
        WHISKER_LOADER_LOGE("unsupported platform — no Lynx SONAME");
        return WHISKER_BRIDGE_LYNX_LOAD_ERR_DLOPEN;
    }

    // RTLD_NOW: surface unresolved symbols immediately. We deliberately
    // do NOT pass RTLD_GLOBAL — the bridge keeps the dlsym scope private
    // to itself; no other library should be reaching for Lynx's symbols
    // through our handle.
    void* handle = dlopen(soname, RTLD_NOW);
    if (handle == nullptr) {
        const char* err = dlerror();
        WHISKER_LOADER_LOGE("dlopen(%s) failed: %s",
                            soname,
                            err != nullptr ? err : "(no dlerror message)");
        return WHISKER_BRIDGE_LYNX_LOAD_ERR_DLOPEN;
    }

    bool ok = true;

    // ABI handshake first — refuse to bind the rest if Lynx ships a
    // version we weren't compiled against. The fork's
    // `lynx_capi_abi_version` is stable across additions, so a
    // mismatch genuinely means a breaking change happened.
    BindSymbol(handle, "lynx_capi_abi_version", &g_capi.abi_version, &ok);
    if (!ok) return WHISKER_BRIDGE_LYNX_LOAD_ERR_MISSING_SYMBOL;
    int32_t found = g_capi.abi_version();
    if (found != WHISKER_LYNX_CAPI_ABI_VERSION) {
        WHISKER_LOADER_LOGE(
            "Lynx C ABI version mismatch: bridge expects %d, Lynx reports %d. "
            "Rebuild Whisker against the matching Lynx fork release.",
            WHISKER_LYNX_CAPI_ABI_VERSION,
            found);
        return WHISKER_BRIDGE_LYNX_LOAD_ERR_ABI_MISMATCH;
    }

    // Shell lifecycle.
    BindSymbol(handle, "lynx_shell_from_native_ptr", &g_capi.shell_from_native_ptr, &ok);
    BindSymbol(handle, "lynx_shell_release", &g_capi.shell_release, &ok);
    BindSymbol(handle, "lynx_shell_run_on_tasm_thread", &g_capi.shell_run_on_tasm_thread, &ok);

    // Element create / release / id.
    BindSymbol(handle, "lynx_create_fiber_element", &g_capi.create_fiber_element, &ok);
    BindSymbol(handle, "lynx_create_fiber_element_by_name", &g_capi.create_fiber_element_by_name, &ok);
    BindSymbol(handle, "lynx_element_release", &g_capi.element_release, &ok);
    BindSymbol(handle, "lynx_element_id", &g_capi.element_id, &ok);

    // Element manipulation.
    BindSymbol(handle, "lynx_element_set_attribute", &g_capi.element_set_attribute, &ok);
    BindSymbol(handle, "lynx_element_set_attribute_int", &g_capi.element_set_attribute_int, &ok);
    BindSymbol(handle, "lynx_element_set_attribute_bool", &g_capi.element_set_attribute_bool, &ok);
    BindSymbol(handle, "lynx_element_set_attribute_double", &g_capi.element_set_attribute_double, &ok);
    BindSymbol(handle, "lynx_element_set_attribute_object", &g_capi.element_set_attribute_object, &ok);
    BindSymbol(handle, "lynx_element_set_inline_styles", &g_capi.element_set_inline_styles, &ok);
    BindSymbol(handle, "lynx_element_set_update_list_info", &g_capi.element_set_update_list_info, &ok);
    BindSymbol(handle, "lynx_element_set_event_handler", &g_capi.element_set_event_handler, &ok);
    BindSymbol(handle, "lynx_element_append_child", &g_capi.element_append_child, &ok);
    BindSymbol(handle, "lynx_element_remove_child", &g_capi.element_remove_child, &ok);
    BindSymbol(handle, "lynx_list_set_native_item_provider", &g_capi.list_set_native_item_provider, &ok);

    // Pipeline.
    BindSymbol(handle, "lynx_shell_set_root_element", &g_capi.shell_set_root_element, &ok);
    BindSymbol(handle, "lynx_shell_flush", &g_capi.shell_flush, &ok);

    // UI method dispatch.
    BindSymbol(handle, "lynx_ui_invoke_method", &g_capi.ui_invoke_method, &ok);
    BindSymbol(handle, "lynx_ui_invoke_method_with_params", &g_capi.ui_invoke_method_with_params, &ok);
    BindSymbol(handle, "lynx_ui_invoke_method_async", &g_capi.ui_invoke_method_async, &ok);
    BindSymbol(handle, "lynx_ui_invoke_method_async_with_params", &g_capi.ui_invoke_method_async_with_params, &ok);

    // Animation.
    BindSymbol(handle, "lynx_element_animate", &g_capi.element_animate, &ok);

    // Core-originated custom events — OPTIONAL (tail-added after ABI v2).
    // A missing symbol is not an error: the field stays NULL and the
    // bridge feature-detects at the install site (list events simply
    // don't fire on an older Lynx, same as before the feature existed).
    (void)dlerror();
    g_capi.shell_set_custom_event_callback =
        reinterpret_cast<lynx_shell_set_custom_event_callback_fn>(
            dlsym(handle, "lynx_shell_set_custom_event_callback"));

    // Explicit list diff actions — part of the ABI v3 surface, so a
    // strict bind like the rest.
    BindSymbol(handle, "lynx_element_update_list_actions", &g_capi.element_update_list_actions, &ok);

    if (!ok) return WHISKER_BRIDGE_LYNX_LOAD_ERR_MISSING_SYMBOL;
    return WHISKER_BRIDGE_LYNX_LOAD_OK;
}

}  // namespace

extern "C" int whisker_bridge_load_lynx(void) {
    std::call_once(g_load_once, []() {
        g_load_result.store(DoLoad(), std::memory_order_release);
    });
    return g_load_result.load(std::memory_order_acquire);
}

extern "C" const WhiskerLynxCapi* whisker_lynx_capi(void) {
    return g_load_result.load(std::memory_order_acquire) == WHISKER_BRIDGE_LYNX_LOAD_OK
               ? &g_capi
               : nullptr;
}
