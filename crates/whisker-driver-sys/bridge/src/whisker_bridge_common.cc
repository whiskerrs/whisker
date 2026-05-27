// whisker_bridge_common.cc
//
// Platform-independent bridge implementation.
//
// All `whisker_bridge_*` C-ABI symbols defined here are safe to call
// from any host (iOS, Android, …). Per-platform plumbing (LynxView
// ivar / JNI field access, event-system hooks) lives in
// whisker_bridge_ios.mm / whisker_bridge_android.cc.
//
// Phase 6-α refactor: this file used to include
// "core/shell/lynx_shell.h" etc. directly and call C++ instance
// methods through reinterpret_cast<LynxShell*>. That required
// patching Lynx to drop -fvisibility=hidden and pulled in mangled-
// symbol fragility. Now every operation goes through Lynx's stable
// extern "C" API (lynx_native_renderer_capi.h). Lynx-side internals
// stay hidden; only the LYNX_CAPI_EXPORT-tagged functions cross the
// .so boundary.

#include <cstdint>
#include <cstring>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

#include "lynx_native_renderer_capi.h"

#include "whisker_bridge.h"
#include "whisker_bridge_internal.h"

// ----------------------------------------------------------------------------
// Internal types
// ----------------------------------------------------------------------------

struct WhiskerEngine {
    // Borrowed Lynx shell handle. Lifetime is bounded by the LynxView the
    // engine was attached to — see WhiskerEngine destruction below.
    lynx_shell_t* shell = nullptr;
    // Set by the platform glue once it has wired its event hook into
    // the host. Only meaningful to the glue layer; common code just
    // stores it.
    bool event_reporter_installed = false;
};

// WhiskerElement wraps an opaque Lynx fiber element handle. The C ABI
// hands out raw pointers; whisker_bridge_release_element drops the
// underlying lynx_fiber_element_t (which itself drops Lynx's strong
// ref).
//
// `shell` is the lynx_shell_t* the element was created against —
// stashed here so element-method dispatch (Phase 7-Φ.H.2.7) can
// reach the shell without threading WhiskerEngine through every
// per-element call. Borrowed; the engine outlives every element
// it spawned.
struct WhiskerElement {
    lynx_fiber_element_t* handle = nullptr;
    lynx_shell_t* shell = nullptr;
};

// Event dispatch hook. Lynx dispatches physical touches through
// `Element::SetEventHandler(EventHandler*)` consumed by
// `TouchEventHandler` (whose ultimate target is the JS runtime), not
// through the EventTarget / AddEventListener path — so we can't just
// hang a `lynx::event::EventListener` off a FiberElement and expect
// taps to fire it. We also can't observe Lynx's native capture/bubble
// CHAIN: each platform's glue installs a "reporter" hook on the host's
// event emitter (LynxEventEmitter on iOS / Android), but that hook
// fires once, at the hit-tested TARGET, *before* the engine walks the
// chain (whose per-element firings go to the absent JS runtime). See
// `whisker/memory/lynx_event_dispatch`.
//
// So Whisker reconstructs propagation itself: the reporter calls
// `whisker_bridge_internal_dispatch_event` with the target sign +
// event body, and that forwards to a dispatcher the Rust driver
// registers (`whisker_bridge_register_event_dispatcher`). The driver
// owns the element tree + the per-element listeners (with their
// bind/catch/capture type) and replays Lynx's capture→bubble→catch
// algorithm in Rust. Returning `true` tells the reporter the event
// was consumed (Lynx then skips its own native chain for it).
namespace {

WhiskerEventDispatcher& EventDispatcher() {
    static WhiskerEventDispatcher dispatcher = nullptr;
    return dispatcher;
}

}  // namespace

// ----------------------------------------------------------------------------
// Engine lifecycle — internal helpers exposed to platform glue
// ----------------------------------------------------------------------------

WhiskerEngine* whisker_bridge_internal_engine_create(void* native_shell_ptr) {
    lynx_shell_t* shell = lynx_shell_from_native_ptr(native_shell_ptr);
    if (shell == nullptr) return nullptr;
    auto* engine = new WhiskerEngine();
    engine->shell = shell;
    return engine;
}

void whisker_bridge_internal_mark_event_reporter_installed(WhiskerEngine* engine) {
    if (engine != nullptr) engine->event_reporter_installed = true;
}

bool whisker_bridge_internal_is_event_reporter_installed(const WhiskerEngine* engine) {
    return engine != nullptr && engine->event_reporter_installed;
}

bool whisker_bridge_internal_dispatch_event(int32_t element_sign,
                                            const char* event_name,
                                            const WhiskerValueRaw* payload) {
    if (event_name == nullptr) return false;
    WhiskerEventDispatcher dispatcher = EventDispatcher();
    if (dispatcher == nullptr) return false;
    // Normalise a NULL payload to a stack `WHISKER_VALUE_NULL` so the
    // dispatcher always receives a valid pointer ("no body").
    WhiskerValueRaw null_value{};
    null_value.type = WHISKER_VALUE_NULL;
    const WhiskerValueRaw* p = payload != nullptr ? payload : &null_value;
    return dispatcher(element_sign, event_name, p);
}

// ----------------------------------------------------------------------------
// Engine lifecycle — public C ABI
// ----------------------------------------------------------------------------

// `whisker_bridge_engine_attach` is platform-specific (lives in
// whisker_bridge_ios.mm / whisker_bridge_android.cc); the common code
// only provides the `release` half.

extern "C" void whisker_bridge_engine_release(WhiskerEngine* engine) {
    if (engine == nullptr) return;
    // The shell owns its own RefPtr to the root page (constructed
    // inside `lynx_shell_set_root_element`), so releasing the shell
    // transitively drops Lynx's reference to it. The WhiskerElement
    // wrapper for the page is freed via its `release_element` call
    // on the Whisker-runtime side, independently of this.
    if (engine->shell != nullptr) {
        lynx_shell_release(engine->shell);
        engine->shell = nullptr;
    }
    delete engine;
}

// ----------------------------------------------------------------------------
// Thread dispatch
// ----------------------------------------------------------------------------

extern "C" bool whisker_bridge_dispatch(WhiskerEngine* engine,
                                       WhiskerTasmCallback callback,
                                       void* user_data) {
    if (engine == nullptr || engine->shell == nullptr || callback == nullptr) {
        return false;
    }
    // Lynx's C API takes the same callback shape — we can pass our
    // user_data + callback through directly. Fiber-arch init happens
    // inside lynx_shell_run_on_tasm_thread on the first call.
    return lynx_shell_run_on_tasm_thread(engine->shell, callback, user_data);
}

// ----------------------------------------------------------------------------
// Element creation / lifetime
// ----------------------------------------------------------------------------

namespace {

lynx_element_tag_e MapTag(WhiskerElementTag tag) {
    switch (tag) {
        case WhiskerElementTagPage:       return LYNX_ELEMENT_TAG_PAGE;
        case WhiskerElementTagView:       return LYNX_ELEMENT_TAG_VIEW;
        case WhiskerElementTagText:       return LYNX_ELEMENT_TAG_TEXT;
        case WhiskerElementTagRawText:    return LYNX_ELEMENT_TAG_RAW_TEXT;
        case WhiskerElementTagImage:      return LYNX_ELEMENT_TAG_IMAGE;
        case WhiskerElementTagScrollView: return LYNX_ELEMENT_TAG_SCROLL_VIEW;
    }
    return LYNX_ELEMENT_TAG_VIEW;
}

}  // namespace

extern "C" WhiskerElement* whisker_bridge_create_element(WhiskerEngine* engine,
                                                        WhiskerElementTag tag) {
    if (engine == nullptr || engine->shell == nullptr) return nullptr;
    lynx_fiber_element_t* handle =
        lynx_create_fiber_element(engine->shell, MapTag(tag));
    if (handle == nullptr) return nullptr;
    return new WhiskerElement{handle, engine->shell};
}

extern "C" WhiskerElement* whisker_bridge_create_element_by_name(
    WhiskerEngine* engine,
    const char* tag_name) {
    if (engine == nullptr || engine->shell == nullptr || tag_name == nullptr) {
        return nullptr;
    }
    lynx_fiber_element_t* handle =
        lynx_create_fiber_element_by_name(engine->shell, tag_name);
    if (handle == nullptr) return nullptr;
    return new WhiskerElement{handle, engine->shell};
}

extern "C" void whisker_bridge_release_element(WhiskerElement* element) {
    if (element == nullptr) return;
    // Listeners + the sign→parent map are owned by the Rust driver's
    // renderer now (it drops them in its own `release_element`, keyed
    // by the same sign), so there's nothing to clean up here.
    if (element->handle != nullptr) {
        lynx_element_release(element->handle);
    }
    delete element;
}

// ----------------------------------------------------------------------------
// Element manipulation
// ----------------------------------------------------------------------------

extern "C" void whisker_bridge_set_attribute(WhiskerElement* element,
                                            const char* key,
                                            const char* value) {
    if (element == nullptr || element->handle == nullptr) return;
    lynx_element_set_attribute(element->handle, key, value);
}

extern "C" void whisker_bridge_set_inline_styles(WhiskerElement* element,
                                                const char* css) {
    if (element == nullptr || element->handle == nullptr) return;
    lynx_element_set_inline_styles(element->handle, css);
}

extern "C" void whisker_bridge_append_child(WhiskerElement* parent,
                                           WhiskerElement* child) {
    if (parent == nullptr || child == nullptr) return;
    lynx_element_append_child(parent->handle, child->handle);
}

extern "C" void whisker_bridge_remove_child(WhiskerElement* parent,
                                           WhiskerElement* child) {
    if (parent == nullptr || child == nullptr) return;
    lynx_element_remove_child(parent->handle, child->handle);
}

// Superseded by Rust-side propagation reconstruction: listeners now
// live in the `whisker-driver` renderer (keyed by element sign, with
// their bind/catch/capture type), and dispatch runs through the
// dispatcher registered below. These two symbols are retained as
// no-ops for ABI stability (the iOS exported-symbols list still names
// them); the Rust driver no longer calls them.
extern "C" void whisker_bridge_set_event_listener(WhiskerElement* element,
                                                 const char* event_name,
                                                 WhiskerEventCallback callback,
                                                 void* user_data) {
    (void)element;
    (void)event_name;
    (void)callback;
    (void)user_data;
}

extern "C" void whisker_bridge_set_event_listener_with_value(
    WhiskerElement* element,
    const char* event_name,
    WhiskerEventValueCallback callback,
    void* user_data) {
    (void)element;
    (void)event_name;
    (void)callback;
    (void)user_data;
}

extern "C" void whisker_bridge_register_event_dispatcher(
    WhiskerEventDispatcher dispatcher) {
    EventDispatcher() = dispatcher;
}

extern "C" int32_t whisker_bridge_element_sign(WhiskerElement* element) {
    if (element == nullptr || element->handle == nullptr) return 0;
    return lynx_element_id(element->handle);
}

extern "C" void whisker_bridge_set_native_event_handler(WhiskerElement* element,
                                                        const char* event_name) {
    if (element == nullptr || element->handle == nullptr || event_name == nullptr) {
        return;
    }
    // Populate the element's Lynx event set so its UI component emits the
    // event (scroll / layout / uiappear / …). The fire is still observed
    // via the reporter → dispatcher path.
    //
    // `lynx_element_set_event_handler` ships in the Lynx fork's liblynx
    // as of v3.7.0-whisker.6 (whiskerrs/lynx#6), so this works on both
    // platforms now.
    lynx_element_set_event_handler(element->handle, event_name);
}

// ----------------------------------------------------------------------------
// Pipeline
// ----------------------------------------------------------------------------

extern "C" void whisker_bridge_set_root(WhiskerEngine* engine, WhiskerElement* page) {
    if (engine == nullptr || engine->shell == nullptr ||
        page == nullptr || page->handle == nullptr) {
        return;
    }
    // `lynx_shell_set_root_element` constructs its own
    // `fml::RefPtr<PageElement>` from the handle (bumping Lynx's
    // intrusive refcount) and stores it inside the shell. After this
    // call the page's underlying FiberElement is kept alive by the
    // shell's strong ref, *independent* of the WhiskerElement's
    // handle.
    //
    // We deliberately do NOT clear `page->handle` here, nor stash an
    // aliased pointer in `engine->root_page`. Both would break
    // subsequent `append_child(page, …)` / `remove_child(page, …)`
    // calls (the null-handle guard in those functions would silently
    // no-op every operation on the root page), which is fatal for
    // the per-component remount path — every hot patch detaches /
    // reattaches body roots on the root page.
    //
    // Shell teardown releases its strong ref via `lynx_shell_release`
    // (in `whisker_bridge_engine_release`); the WhiskerElement's ref
    // is released via the normal `release_element` path when the
    // root owner is disposed. The two refs never share ownership of
    // the `lynx_fiber_element_t` wrapper, so there's no double-free.
    lynx_shell_set_root_element(engine->shell, page->handle);
}

extern "C" void whisker_bridge_flush(WhiskerEngine* engine) {
    if (engine == nullptr || engine->shell == nullptr) return;
    lynx_shell_flush(engine->shell);
}

// ---- Native module invocation (Phase 7-Φ.F) -------------------------------
//
// Pure-C dispatch on iOS / host: a `(module_name →
// WhiskerModuleDispatchFn)` table the platform-side generated code
// populates at app launch. `whisker_bridge_invoke_module` resolves
// the dispatch fn by name and calls it; `value_release` walks the
// recursive WhiskerValue tree and frees any heap allocations the
// dispatch produced.
//
// On Android, `whisker_bridge_invoke_module` is overridden in
// `whisker_bridge_android.cc` to go through JNI into Kotlin's
// `WhiskerModuleRegistry.invokeDispatch(...)`. That keeps the
// per-module dispatch class in Kotlin (where KSP generates it)
// rather than requiring per-module C thunks. Hence the
// `#if !defined(__ANDROID__)` guard around `invoke_module` /
// `invoke_module_async` below.
//
// `register_module_dispatch` and `value_release` stay shared —
// register is used on iOS only (Android registers via Kotlin
// side), but defining it everywhere keeps the C ABI symmetric.

#include <cstring>
#include <cstdlib>
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

WhiskerValueRaw MakeBridgeErrorValue(const char* message) {
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

#if !defined(__ANDROID__)
extern "C" WhiskerValueRaw whisker_bridge_invoke_module(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count) {
    if (module_name == nullptr || method_name == nullptr) {
        return MakeBridgeErrorValue("module/method name is NULL");
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
        return MakeBridgeErrorValue("module not registered");
    }
    return fn(method_name, args, arg_count);
}

extern "C" bool whisker_bridge_invoke_module_async(
    const char* module_name,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (callback == nullptr) return false;
    // Foundation: sync-forward on the calling thread. Worker-pool
    // dispatch + cancel semantics land alongside the first
    // async-API module (out of Phase F scope).
    WhiskerValueRaw result = whisker_bridge_invoke_module(
        module_name, method_name, args, arg_count);
    callback(user_data, &result);
    whisker_bridge_value_release(&result);
    return true;
}
#endif  // !__ANDROID__

// Phase 7-Φ.H.2.7 — `whisker_bridge_invoke_element_method` impl.
//
// Routes element-method calls (`video.play()`, etc.) through the
// fork's `lynx_ui_invoke_method` wrapper, which packages args as
// `{"args": [arg0, arg1, ...]}` and calls
// `Catalyzer::Invoke(sign, method, params, callback)`. That in
// turn dispatches via `LynxUIMethodProcessor.invokeMethod:` (iOS)
// or `LynxUIMethodsExecutor.invokeMethod(...)` (Android) onto the
// `@WhiskerUIMethod`-emitted forwarder on the mounted element's
// `WhiskerUI<View>` subclass.
//
// `element->shell` carries the `lynx_shell_t*` we need; the sign
// comes from `lynx_element_id(element->handle)`.
//
// Currently fire-and-forget — the platform Invoke routes the
// actual call to the main / UI thread, so the result isn't
// available synchronously. Typed Rust wrappers (`fn play(&self)`)
// discard the result anyway, so this matches v1's contract.
// Returns `WHISKER_VALUE_NULL` on dispatch-scheduled-OK, or an
// Error if preconditions fail (NULL element, NULL shell, NUL byte
// in method name, manager not initialised).
// Convert WhiskerValueRaw[] → lynx_ui_method_value_t[]. Lynx owns
// string buffers transiently — they're copied into `base::String`
// inside the wrapper before returning, so the pointers we hand over
// only need to outlive the call. Arrays / maps / bytes / errors
// aren't representable in the scalar `lynx_ui_method_value_t` arg ABI
// (they'd need a recursive lepus builder); treated as NULL.
static void BuildLynxUiArgs(const WhiskerValueRaw* args, size_t arg_count,
                            std::vector<lynx_ui_method_value_t>& lynx_args) {
    lynx_args.reserve(arg_count);
    for (size_t i = 0; i < arg_count; i++) {
        lynx_ui_method_value_t out;
        std::memset(&out, 0, sizeof(out));
        const WhiskerValueRaw& v = args[i];
        switch (v.type) {
            case WHISKER_VALUE_BOOL:
                out.type = LYNX_UI_METHOD_VALUE_BOOL;
                out.v.b = v.v.b;
                break;
            case WHISKER_VALUE_INT:
                out.type = LYNX_UI_METHOD_VALUE_INT;
                out.v.i = v.v.i;
                break;
            case WHISKER_VALUE_FLOAT:
                out.type = LYNX_UI_METHOD_VALUE_DOUBLE;
                out.v.f = v.v.f;
                break;
            case WHISKER_VALUE_STRING:
                out.type = LYNX_UI_METHOD_VALUE_STRING;
                out.v.s = v.v.s.ptr;  // borrowed for the call
                break;
            default:
                out.type = LYNX_UI_METHOD_VALUE_NULL;
                break;
        }
        lynx_args.push_back(out);
    }
}

extern "C" WhiskerValueRaw whisker_bridge_invoke_element_method(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count) {
    if (element == nullptr || element->handle == nullptr ||
        element->shell == nullptr || method_name == nullptr) {
        return MakeBridgeErrorValue(
            "whisker_bridge_invoke_element_method: NULL element / shell / method");
    }
    int32_t sign = lynx_element_id(element->handle);
    if (sign <= 0) {
        return MakeBridgeErrorValue(
            "whisker_bridge_invoke_element_method: element has no sign yet "
            "(was it flushed into the tree?)");
    }

    std::vector<lynx_ui_method_value_t> lynx_args;
    BuildLynxUiArgs(args, arg_count, lynx_args);

    int32_t code = lynx_ui_invoke_method(
        element->shell, sign, method_name,
        lynx_args.empty() ? nullptr : lynx_args.data(), lynx_args.size());
    if (code != 0) {
        return MakeBridgeErrorValue(
            ("lynx_ui_invoke_method returned non-zero (code=" +
             std::to_string(code) + ")").c_str());
    }
    // Success: dispatch was scheduled. Return Null since the actual
    // method's return value isn't synchronously available.
    WhiskerValueRaw ok;
    std::memset(&ok, 0, sizeof(ok));
    ok.type = WHISKER_VALUE_NULL;
    return ok;
}

namespace {
// Deep-copy a Lynx-neutral `lynx_ui_method_value_t` result tree into a
// `WhiskerValueRaw` (heap-owned, matching `whisker_bridge_value_release`'s
// free pattern). The capi tree itself is owned + freed by
// `lynx_native_renderer.cc` after the callback returns.
WhiskerValueRaw CapiValueToWhisker(const lynx_ui_method_value_t* v) {
    WhiskerValueRaw out;
    std::memset(&out, 0, sizeof(out));
    if (v == nullptr) {
        out.type = WHISKER_VALUE_NULL;
        return out;
    }
    switch (v->type) {
        case LYNX_UI_METHOD_VALUE_BOOL:
            out.type = WHISKER_VALUE_BOOL;
            out.v.b = v->v.b;
            break;
        case LYNX_UI_METHOD_VALUE_INT:
            out.type = WHISKER_VALUE_INT;
            out.v.i = v->v.i;
            break;
        case LYNX_UI_METHOD_VALUE_DOUBLE:
            out.type = WHISKER_VALUE_FLOAT;
            out.v.f = v->v.f;
            break;
        case LYNX_UI_METHOD_VALUE_STRING: {
            const char* s = v->v.s != nullptr ? v->v.s : "";
            size_t len = std::strlen(s);
            char* buf = static_cast<char*>(std::malloc(len == 0 ? 1 : len));
            if (len > 0) std::memcpy(buf, s, len);
            out.type = WHISKER_VALUE_STRING;
            out.v.s.ptr = buf;
            out.v.s.len = len;
            break;
        }
        case LYNX_UI_METHOD_VALUE_ARRAY: {
            size_t n = v->v.array.count;
            out.type = WHISKER_VALUE_ARRAY;
            out.v.array.count = n;
            out.v.array.items = n > 0 ? static_cast<WhiskerValueRaw*>(
                                            std::malloc(sizeof(WhiskerValueRaw) * n))
                                      : nullptr;
            for (size_t i = 0; i < n; i++) {
                out.v.array.items[i] = CapiValueToWhisker(&v->v.array.items[i]);
            }
            break;
        }
        case LYNX_UI_METHOD_VALUE_MAP: {
            size_t n = v->v.map.count;
            out.type = WHISKER_VALUE_MAP;
            out.v.map.count = n;
            out.v.map.entries = n > 0 ? static_cast<WhiskerKeyValueRaw*>(
                                            std::malloc(sizeof(WhiskerKeyValueRaw) * n))
                                      : nullptr;
            for (size_t i = 0; i < n; i++) {
                const char* k = v->v.map.entries[i].key != nullptr
                                    ? v->v.map.entries[i].key
                                    : "";
                size_t klen = std::strlen(k);
                char* kbuf = static_cast<char*>(std::malloc(klen == 0 ? 1 : klen));
                if (klen > 0) std::memcpy(kbuf, k, klen);
                out.v.map.entries[i].key.ptr = kbuf;
                out.v.map.entries[i].key.len = klen;
                out.v.map.entries[i].value = CapiValueToWhisker(&v->v.map.entries[i].value);
            }
            break;
        }
        default:
            out.type = WHISKER_VALUE_NULL;
            break;
    }
    return out;
}

// Carries the Rust callback + user_data across `lynx_ui_invoke_method_
// async`'s `(code, result, user_data)` callback shape down to the
// bridge's `(user_data, result)` shape. Heap-allocated, freed after
// the (single) result callback fires.
struct ElementMethodAsyncCtx {
    WhiskerModuleCallback rust_cb;
    void* rust_user_data;
};

void element_method_async_adapter(int32_t /*code*/,
                                  const lynx_ui_method_value_t* capi_result,
                                  void* user_data) {
    auto* ctx = static_cast<ElementMethodAsyncCtx*>(user_data);
    if (ctx == nullptr) return;
    if (ctx->rust_cb != nullptr) {
        WhiskerValueRaw result = CapiValueToWhisker(capi_result);
        ctx->rust_cb(ctx->rust_user_data, &result);
        whisker_bridge_value_release(&result);
    }
    delete ctx;
}

// Synchronously hand `callback` a `WHISKER_VALUE_ERROR(message)` and
// release it. For precondition / unsupported-platform failures.
void FailElementMethodAsync(WhiskerModuleCallback callback, void* user_data,
                            const char* message) {
    if (callback == nullptr) return;
    WhiskerValueRaw err = MakeBridgeErrorValue(message);
    callback(user_data, &err);
    whisker_bridge_value_release(&err);
}
}  // namespace

extern "C" bool whisker_bridge_invoke_element_method_async(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (element == nullptr || element->handle == nullptr ||
        element->shell == nullptr || method_name == nullptr) {
        FailElementMethodAsync(
            callback, user_data,
            "whisker_bridge_invoke_element_method_async: NULL element / shell / method");
        return false;
    }
    int32_t sign = lynx_element_id(element->handle);
    if (sign <= 0) {
        FailElementMethodAsync(
            callback, user_data,
            "whisker_bridge_invoke_element_method_async: element has no sign yet");
        return false;
    }

    // `lynx_ui_invoke_method_async` is exported by liblynx.so on
    // Android (Lynx fork v3.7.0-whisker.5+) and compiled into
    // WhiskerDriver on iOS — same dispatch on both platforms.
    std::vector<lynx_ui_method_value_t> lynx_args;
    BuildLynxUiArgs(args, arg_count, lynx_args);
    auto* ctx = new ElementMethodAsyncCtx{callback, user_data};
    int32_t code = lynx_ui_invoke_method_async(
        element->shell, sign, method_name,
        lynx_args.empty() ? nullptr : lynx_args.data(), lynx_args.size(),
        element_method_async_adapter, ctx);
    if (code != 0) {
        delete ctx;
        FailElementMethodAsync(
            callback, user_data,
            ("lynx_ui_invoke_method_async returned non-zero (code=" +
             std::to_string(code) + ")")
                .c_str());
        return false;
    }
    return true;
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
