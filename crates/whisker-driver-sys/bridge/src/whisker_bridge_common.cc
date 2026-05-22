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
#include <mutex>
#include <string>
#include <unordered_map>

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
struct WhiskerElement {
    lynx_fiber_element_t* handle = nullptr;
};

// Native event listener registry. Lynx dispatches physical touches
// through `Element::SetEventHandler(EventHandler*)` consumed by
// `TouchEventHandler` (whose ultimate target is the JS runtime), not
// through the EventTarget / AddEventListener path — so we can't just
// hang a `lynx::event::EventListener` off a FiberElement and expect
// taps to fire it.
//
// Instead, each platform's glue installs a "reporter" hook on the
// host's event emitter (LynxEventEmitter on iOS). The hook calls
// `whisker_bridge_internal_dispatch_event` below; that looks up
// (element_sign, event_name) here and fires the C callback if present.
namespace {

struct EventKey {
    int32_t element_sign;
    std::string event_name;
    bool operator==(const EventKey& other) const {
        return element_sign == other.element_sign && event_name == other.event_name;
    }
};
struct EventKeyHash {
    size_t operator()(const EventKey& k) const noexcept {
        return std::hash<int32_t>{}(k.element_sign) ^
               (std::hash<std::string>{}(k.event_name) << 1);
    }
};
// Tagged union covering both the no-payload event-listener
// (`whisker_bridge_set_event_listener`) and the string-payload variant
// (`whisker_bridge_set_event_listener_with_payload`). Each registry
// slot is one or the other; dispatch picks the right arm based on
// `kind`.
struct EventCallback {
    enum class Kind {
        NoPayload,
        StringPayload,
    };
    Kind kind;
    union {
        WhiskerEventCallback no_payload;
        WhiskerEventPayloadCallback string_payload;
    } func;
    void* user_data;
};

std::mutex& RegistryMutex() {
    static std::mutex m;
    return m;
}
std::unordered_map<EventKey, EventCallback, EventKeyHash>& Registry() {
    static std::unordered_map<EventKey, EventCallback, EventKeyHash> r;
    return r;
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
                                            const char* payload_json) {
    if (event_name == nullptr) return false;
    EventCallback hit{};
    bool found = false;
    {
        std::lock_guard<std::mutex> lock(RegistryMutex());
        EventKey key{element_sign, std::string(event_name)};
        auto it = Registry().find(key);
        if (it != Registry().end()) {
            hit = it->second;
            found = true;
        }
    }
    if (!found) return false;
    // `payload_json` is the bridge's contract — it normalises NULL
    // to the empty string so callbacks never have to NULL-check.
    const char* payload = payload_json != nullptr ? payload_json : "";
    switch (hit.kind) {
        case EventCallback::Kind::NoPayload:
            if (hit.func.no_payload != nullptr) {
                hit.func.no_payload(hit.user_data);
                return true;
            }
            return false;
        case EventCallback::Kind::StringPayload:
            if (hit.func.string_payload != nullptr) {
                hit.func.string_payload(hit.user_data, payload);
                return true;
            }
            return false;
    }
    return false;
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
    return new WhiskerElement{handle};
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
    return new WhiskerElement{handle};
}

extern "C" void whisker_bridge_release_element(WhiskerElement* element) {
    if (element == nullptr) return;
    // Drop any registered native event callbacks for this element so
    // its sign can't accidentally collide with a future element's id.
    if (element->handle != nullptr) {
        int32_t sign = lynx_element_id(element->handle);
        std::lock_guard<std::mutex> lock(RegistryMutex());
        for (auto it = Registry().begin(); it != Registry().end(); ) {
            if (it->first.element_sign == sign) {
                it = Registry().erase(it);
            } else {
                ++it;
            }
        }
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

extern "C" void whisker_bridge_set_event_listener(WhiskerElement* element,
                                                 const char* event_name,
                                                 WhiskerEventCallback callback,
                                                 void* user_data) {
    if (element == nullptr || element->handle == nullptr ||
        event_name == nullptr || callback == nullptr) {
        return;
    }
    EventKey key{lynx_element_id(element->handle), std::string(event_name)};
    EventCallback entry{};
    entry.kind = EventCallback::Kind::NoPayload;
    entry.func.no_payload = callback;
    entry.user_data = user_data;
    std::lock_guard<std::mutex> lock(RegistryMutex());
    Registry()[key] = entry;
}

extern "C" void whisker_bridge_set_event_listener_with_payload(
    WhiskerElement* element,
    const char* event_name,
    WhiskerEventPayloadCallback callback,
    void* user_data) {
    if (element == nullptr || element->handle == nullptr ||
        event_name == nullptr || callback == nullptr) {
        return;
    }
    EventKey key{lynx_element_id(element->handle), std::string(event_name)};
    EventCallback entry{};
    entry.kind = EventCallback::Kind::StringPayload;
    entry.func.string_payload = callback;
    entry.user_data = user_data;
    std::lock_guard<std::mutex> lock(RegistryMutex());
    Registry()[key] = entry;
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
