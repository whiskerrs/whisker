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
    // Strong reference to the installed root page (so Lynx doesn't
    // drop it while we still own the engine).
    lynx_fiber_element_t* root_page = nullptr;
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
struct EventCallback {
    WhiskerEventCallback callback;
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
                                            const char* event_name) {
    if (event_name == nullptr) return false;
    EventCallback hit{nullptr, nullptr};
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
    if (found && hit.callback) {
        hit.callback(hit.user_data);
        return true;
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
    // The root page keeps a reference into Lynx; drop it first so the
    // shell can clean up cleanly even if the caller has torn down the
    // LynxView already.
    if (engine->root_page != nullptr) {
        lynx_element_release(engine->root_page);
        engine->root_page = nullptr;
    }
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
    std::lock_guard<std::mutex> lock(RegistryMutex());
    Registry()[key] = EventCallback{callback, user_data};
}

// ----------------------------------------------------------------------------
// Pipeline
// ----------------------------------------------------------------------------

extern "C" void whisker_bridge_set_root(WhiskerEngine* engine, WhiskerElement* page) {
    if (engine == nullptr || engine->shell == nullptr ||
        page == nullptr || page->handle == nullptr) {
        return;
    }
    // Take over the page's Lynx handle. The engine now owns the
    // strong ref; clear the WhiskerElement's handle so subsequent
    // release_element is a no-op (the caller's WhiskerElement* is
    // still safe to call release on, just won't double-free).
    if (engine->root_page != nullptr) {
        lynx_element_release(engine->root_page);
    }
    engine->root_page = page->handle;
    page->handle = nullptr;
    lynx_shell_set_root_element(engine->shell, engine->root_page);
}

extern "C" void whisker_bridge_flush(WhiskerEngine* engine) {
    if (engine == nullptr || engine->shell == nullptr) return;
    lynx_shell_flush(engine->shell);
}
