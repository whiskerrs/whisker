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
// extern "C" API. Lynx-side internals stay hidden; only the
// LYNX_CAPI_EXPORT-tagged functions cross the .so boundary.
//
// Step-6 refactor (build decoupling): the bridge no longer carries
// link-time UND refs to `lynx_*` symbols. `lynx_capi.h` declares
// function pointer typedefs + a `WhiskerLynxCapi` dispatch struct;
// `whisker_bridge_load_lynx()` dlopens Lynx at engine_attach time
// and fills the struct, and every call site here goes through
// `whisker_lynx_capi()->fn(args)`. That lets the user crate's
// `cargo build` succeed without a prior `whisker build` to fetch
// the Lynx artifacts.

#include <atomic>
#include <cstdint>
#include <cstring>
#include <deque>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

#if defined(__ANDROID__)
#include <android/log.h>
#elif defined(__APPLE__)
#include <os/log.h>
#endif

#include "lynx_capi.h"

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
    // Bind Lynx's C ABI before the first dispatch through it. Idempotent:
    // subsequent attaches see the cached success and short-circuit. A
    // non-zero return means dlopen / dlsym / ABI handshake failed — bail
    // out so the NULL dispatch table doesn't crash on the next line.
    if (whisker_bridge_load_lynx() != 0) return nullptr;
    lynx_shell_t* shell = whisker_lynx_capi()->shell_from_native_ptr(native_shell_ptr);
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
        whisker_lynx_capi()->shell_release(engine->shell);
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
    return whisker_lynx_capi()->shell_run_on_tasm_thread(engine->shell, callback, user_data);
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
        case WhiskerElementTagScrollView: return LYNX_ELEMENT_TAG_SCROLL_VIEW;
    }
    return LYNX_ELEMENT_TAG_VIEW;
}

}  // namespace

extern "C" WhiskerElement* whisker_bridge_create_element(WhiskerEngine* engine,
                                                        WhiskerElementTag tag) {
    if (engine == nullptr || engine->shell == nullptr) return nullptr;
    lynx_fiber_element_t* handle =
        whisker_lynx_capi()->create_fiber_element(engine->shell, MapTag(tag));
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
        whisker_lynx_capi()->create_fiber_element_by_name(engine->shell, tag_name);
    if (handle == nullptr) return nullptr;
    return new WhiskerElement{handle, engine->shell};
}

extern "C" void whisker_bridge_release_element(WhiskerElement* element) {
    if (element == nullptr) return;
    // Listeners + the sign→parent map are owned by the Rust driver's
    // renderer now (it drops them in its own `release_element`, keyed
    // by the same sign), so there's nothing to clean up here.
    if (element->handle != nullptr) {
        whisker_lynx_capi()->element_release(element->handle);
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
    whisker_lynx_capi()->element_set_attribute(element->handle, key, value);
}

// Typed-attribute variants — Lynx's prop dispatch on many UIs
// (e.g. decoupled `<list>` reading `span-count` / `column-count`)
// gates branches on `value.IsNumber()` / `value.IsBool()`, so a
// stringified attr from `whisker_bridge_set_attribute` silently
// no-ops for those props. These forward to the matching typed
// `lynx_element_set_attribute_*` capi so the dispatch sees the
// right `lepus::Value` discriminant.
extern "C" void whisker_bridge_set_attribute_int(WhiskerElement* element,
                                                 const char* key,
                                                 int64_t value) {
    if (element == nullptr || element->handle == nullptr) return;
    whisker_lynx_capi()->element_set_attribute_int(element->handle, key, value);
}

extern "C" void whisker_bridge_set_attribute_bool(WhiskerElement* element,
                                                  const char* key,
                                                  bool value) {
    if (element == nullptr || element->handle == nullptr) return;
    whisker_lynx_capi()->element_set_attribute_bool(element->handle, key, value);
}

extern "C" void whisker_bridge_set_attribute_double(WhiskerElement* element,
                                                    const char* key,
                                                    double value) {
    if (element == nullptr || element->handle == nullptr) return;
    whisker_lynx_capi()->element_set_attribute_double(element->handle, key, value);
}

// Feed a `<list>` element its item-count so Lynx's decoupled native
// list can build its `update-list-info` map of positional item-keys.
// Called by the `list` builder's `__h()` finalize after all children
// have been appended; the builder also writes the matching `item-key`
// attr (`w_<i>`) onto each child via `child()`.
extern "C" void whisker_bridge_list_set_item_count(WhiskerElement* element,
                                                  int32_t count) {
    if (element == nullptr || element->handle == nullptr) return;
    whisker_lynx_capi()->element_set_update_list_info(element->handle, count);
}

// Install a native item provider on a `<list>` element so Whisker can
// drive Lynx's list virtualisation directly. On both platforms this
// resolves to Lynx's `lynx_list_set_native_item_provider` (Android:
// inside liblynx.so; iOS: inside Lynx.framework, fork-built since
// v3.7.0-whisker.21 — whiskerrs/lynx#19). The boxed closures the
// Rust trampoline hands off are owned by the C++ ListElement via a
// `std::shared_ptr` with `user_data_free` as deleter.
extern "C" void whisker_bridge_list_set_native_item_provider(
    WhiskerElement* element,
    int32_t (*component_at_index)(uint32_t index, int64_t operation_id,
                                  int reuse_notification, void* user_data),
    void (*enqueue_component)(int32_t sign, void* user_data),
    void* user_data,
    void (*user_data_free)(void* user_data)) {
    if (element == nullptr || element->handle == nullptr) {
        if (user_data != nullptr && user_data_free != nullptr) {
            user_data_free(user_data);
        }
        return;
    }
    whisker_lynx_capi()->list_set_native_item_provider(element->handle, component_at_index,
                                       enqueue_component, user_data,
                                       user_data_free);
}

extern "C" void whisker_bridge_set_inline_styles(WhiskerElement* element,
                                                const char* css) {
    if (element == nullptr || element->handle == nullptr) return;
    whisker_lynx_capi()->element_set_inline_styles(element->handle, css);
}

extern "C" void whisker_bridge_append_child(WhiskerElement* parent,
                                           WhiskerElement* child) {
    if (parent == nullptr || child == nullptr) return;
    whisker_lynx_capi()->element_append_child(parent->handle, child->handle);
}

extern "C" void whisker_bridge_remove_child(WhiskerElement* parent,
                                           WhiskerElement* child) {
    if (parent == nullptr || child == nullptr) return;
    whisker_lynx_capi()->element_remove_child(parent->handle, child->handle);
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
    return whisker_lynx_capi()->element_id(element->handle);
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
    whisker_lynx_capi()->element_set_event_handler(element->handle, event_name);
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
    whisker_lynx_capi()->shell_set_root_element(engine->shell, page->handle);
}

extern "C" void whisker_bridge_flush(WhiskerEngine* engine) {
    if (engine == nullptr || engine->shell == nullptr) return;
    whisker_lynx_capi()->shell_flush(engine->shell);
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

// ============================================================================
// Module event subscription (Phase L-2c)
// ============================================================================
//
// Listener registry is keyed on `(module, event)` and tracks an
// ordered list of `(listener_id, callback, user_data)` triples.
// `listener_id` is a monotonically-increasing positive integer; we
// expose it to the Rust wrapper so `ModuleSubscription::drop()` can
// O(1)-locate the entry to delete without re-keying by module/event.
//
// Observer hooks (`OnStartObserving` / `OnStopObserving` parallels)
// fire on the 0↔1 listener-count transition for each
// `(module, event)` pair. The native module registers them once at
// load time; the bridge looks them up by module name on every
// add/remove and calls them inline so the native source can spin up
// before the next `send_event` arrives.

namespace {

struct ModuleEventListener {
    int32_t id = 0;
    std::string module_name;
    std::string event_name;
    WhiskerModuleEventCallback callback = nullptr;
    void* user_data = nullptr;
};

struct ModuleObserverHooks {
    WhiskerModuleObserverHook started = nullptr;
    WhiskerModuleObserverHook stopped = nullptr;
};

std::mutex& EventRegistryMutex() {
    static std::mutex m;
    return m;
}

// Listener storage. Keyed by id so removal is O(1); a secondary
// per-(module, event) listener-count map lets us decide whether to
// fire OnStart / OnStop on each transition without rescanning.
std::unordered_map<int32_t, ModuleEventListener>& EventListeners() {
    static std::unordered_map<int32_t, ModuleEventListener> m;
    return m;
}
std::unordered_map<std::string, int>& ListenerCounts() {
    static std::unordered_map<std::string, int> m;
    return m;
}
std::unordered_map<std::string, ModuleObserverHooks>& ObserverHooks() {
    static std::unordered_map<std::string, ModuleObserverHooks> m;
    return m;
}

std::string EventKey(const std::string& module_name, const std::string& event_name) {
    // `\x1f` (unit separator) is forbidden inside identifier-like
    // module / event names, so this concatenation is unambiguous.
    return module_name + "\x1f" + event_name;
}

int32_t NextListenerId() {
    static std::atomic<int32_t> counter{1};
    return counter.fetch_add(1, std::memory_order_relaxed);
}

}  // namespace

extern "C" void whisker_bridge_log_info(const char* tag, const char* msg) {
    if (msg == nullptr) return;
#if defined(__ANDROID__)
    __android_log_print(ANDROID_LOG_INFO,
                        tag != nullptr ? tag : "WhiskerRust", "%s", msg);
#elif defined(__APPLE__)
    os_log(OS_LOG_DEFAULT, "[%{public}s] %{public}s",
           tag != nullptr ? tag : "WhiskerRust", msg);
#else
    (void)tag;
    (void)msg;
#endif
}

extern "C" int32_t whisker_bridge_module_add_event_listener(
    const char* module_name,
    const char* event_name,
    WhiskerModuleEventCallback callback,
    void* user_data) {
    if (module_name == nullptr || event_name == nullptr || callback == nullptr) {
        return 0;
    }
    int32_t id = NextListenerId();
    std::string module_str(module_name);
    std::string event_str(event_name);
    std::string key = EventKey(module_str, event_str);

    WhiskerModuleObserverHook start_hook = nullptr;
    bool became_first = false;
    {
        std::lock_guard<std::mutex> g(EventRegistryMutex());
        EventListeners()[id] = ModuleEventListener{
            id, module_str, event_str, callback, user_data};
        int& count = ListenerCounts()[key];
        if (count == 0) {
            became_first = true;
            auto it = ObserverHooks().find(module_str);
            if (it != ObserverHooks().end()) {
                start_hook = it->second.started;
            }
        }
        count += 1;
    }
    if (became_first && start_hook != nullptr) {
        // Fire outside the lock — the hook may call back into the
        // bridge (e.g. register an `OnBackInvokedCallback` that
        // synchronously emits a startup event).
        start_hook(module_str.c_str(), event_str.c_str());
    }
    return id;
}

extern "C" void whisker_bridge_module_remove_event_listener(int32_t listener_id) {
    if (listener_id <= 0) return;
    WhiskerModuleObserverHook stop_hook = nullptr;
    std::string module_for_stop;
    std::string event_for_stop;
    {
        std::lock_guard<std::mutex> g(EventRegistryMutex());
        auto it = EventListeners().find(listener_id);
        if (it == EventListeners().end()) return;
        std::string module_str = it->second.module_name;
        std::string event_str = it->second.event_name;
        EventListeners().erase(it);
        std::string key = EventKey(module_str, event_str);
        int& count = ListenerCounts()[key];
        if (count > 0) {
            count -= 1;
            if (count == 0) {
                ListenerCounts().erase(key);
                auto hook_it = ObserverHooks().find(module_str);
                if (hook_it != ObserverHooks().end()) {
                    stop_hook = hook_it->second.stopped;
                    module_for_stop = module_str;
                    event_for_stop = event_str;
                }
            }
        }
    }
    if (stop_hook != nullptr) {
        stop_hook(module_for_stop.c_str(), event_for_stop.c_str());
    }
}

extern "C" void whisker_bridge_module_send_event(
    const char* module_name,
    const char* event_name,
    const WhiskerValueRaw* payload) {
    if (module_name == nullptr || event_name == nullptr) return;
    // Snapshot the listener list under the lock — we don't want to
    // hold it across the user callbacks (which may register / drop
    // further listeners). Vector copy is cheap; listener counts in
    // practice are small (1–2 per gesture / observer).
    std::vector<std::pair<WhiskerModuleEventCallback, void*>> snapshot;
    {
        std::lock_guard<std::mutex> g(EventRegistryMutex());
        for (const auto& kv : EventListeners()) {
            const auto& l = kv.second;
            if (l.module_name == module_name && l.event_name == event_name) {
                snapshot.emplace_back(l.callback, l.user_data);
            }
        }
    }
    for (auto& entry : snapshot) {
        entry.first(entry.second, payload);
    }
}

extern "C" void whisker_bridge_module_register_observer_hooks(
    const char* module_name,
    WhiskerModuleObserverHook started,
    WhiskerModuleObserverHook stopped) {
    if (module_name == nullptr) return;
    std::lock_guard<std::mutex> g(EventRegistryMutex());
    if (started == nullptr && stopped == nullptr) {
        ObserverHooks().erase(module_name);
    } else {
        ObserverHooks()[module_name] = ModuleObserverHooks{started, stopped};
    }
}

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
// comes from `whisker_lynx_capi()->element_id(element->handle)`.
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
    int32_t sign = whisker_lynx_capi()->element_id(element->handle);
    if (sign <= 0) {
        return MakeBridgeErrorValue(
            "whisker_bridge_invoke_element_method: element has no sign yet "
            "(was it flushed into the tree?)");
    }

    std::vector<lynx_ui_method_value_t> lynx_args;
    BuildLynxUiArgs(args, arg_count, lynx_args);

    int32_t code = whisker_lynx_capi()->ui_invoke_method(
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
// Recursive `WhiskerValueRaw` → `lynx_ui_method_value_t` for the
// params-map path. Unlike `BuildLynxUiArgs` (scalar-only), this carries
// maps / arrays through so built-in methods see their nested params.
//
// `value_t` strings / keys are borrowed `const char*` that must be
// NUL-terminated and outlive the capi call; `array.items` / `map.entries`
// are borrowed pointers into contiguous storage. The `CapiArena` owns
// all of it: NUL-terminated `std::string` copies plus per-node child
// vectors. A `std::deque` keeps element addresses stable as nested
// nodes append, and `reserve()` keeps each child vector's buffer fixed
// while it fills — so every `const char*` / pointer we hand over stays
// valid until the arena drops, which is after the (synchronous) capi
// value_t → lepus conversion has copied everything out.
struct CapiArena {
    std::deque<std::string> strings;
    std::deque<std::vector<lynx_ui_method_value_t>> arrays;
    std::deque<std::vector<lynx_ui_method_kv_t>> maps;
};

lynx_ui_method_value_t BuildCapiParamValue(const WhiskerValueRaw& v,
                                           CapiArena& arena) {
    lynx_ui_method_value_t out;
    std::memset(&out, 0, sizeof(out));
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
        case WHISKER_VALUE_STRING: {
            arena.strings.emplace_back(
                v.v.s.ptr != nullptr ? v.v.s.ptr : "", v.v.s.len);
            out.type = LYNX_UI_METHOD_VALUE_STRING;
            out.v.s = arena.strings.back().c_str();
            break;
        }
        case WHISKER_VALUE_ARRAY: {
            arena.arrays.emplace_back();
            std::vector<lynx_ui_method_value_t>& items = arena.arrays.back();
            items.reserve(v.v.array.count);
            for (size_t i = 0; i < v.v.array.count; i++) {
                items.push_back(BuildCapiParamValue(v.v.array.items[i], arena));
            }
            out.type = LYNX_UI_METHOD_VALUE_ARRAY;
            out.v.array.items = items.empty() ? nullptr : items.data();
            out.v.array.count = items.size();
            break;
        }
        case WHISKER_VALUE_MAP: {
            arena.maps.emplace_back();
            std::vector<lynx_ui_method_kv_t>& entries = arena.maps.back();
            entries.reserve(v.v.map.count);
            for (size_t i = 0; i < v.v.map.count; i++) {
                const WhiskerKeyValueRaw& src = v.v.map.entries[i];
                arena.strings.emplace_back(
                    src.key.ptr != nullptr ? src.key.ptr : "", src.key.len);
                lynx_ui_method_kv_t kv;
                std::memset(&kv, 0, sizeof(kv));
                kv.key = arena.strings.back().c_str();
                kv.value = BuildCapiParamValue(src.value, arena);
                entries.push_back(kv);
            }
            out.type = LYNX_UI_METHOD_VALUE_MAP;
            out.v.map.entries = entries.empty() ? nullptr : entries.data();
            out.v.map.count = entries.size();
            break;
        }
        default:
            out.type = LYNX_UI_METHOD_VALUE_NULL;
            break;
    }
    return out;
}
}  // namespace

// -------- Element-level animation dispatch ---------------------------------
//
// Thin wrapper around `lynx_element_animate` (Lynx fork's new capi). Routes
// `keyframes` / `options` through the same `BuildCapiParamValue` arena the
// `_with_params` element-method path uses, so the C++ side gets a single
// recursive `lynx_ui_method_value_t` for each. `keyframes` / `options` may be
// NULL (PLAY / PAUSE / CANCEL / FINISH only need `animation_name`).
extern "C" WhiskerValueRaw whisker_bridge_element_animate(
    WhiskerElement* element,
    int32_t operation,
    const char* animation_name,
    const WhiskerValueRaw* keyframes,
    const WhiskerValueRaw* options) {
    if (element == nullptr || element->handle == nullptr ||
        element->shell == nullptr) {
        return MakeBridgeErrorValue(
            "whisker_bridge_element_animate: NULL element / shell");
    }
    CapiArena arena;
    lynx_ui_method_value_t kf{};
    lynx_ui_method_value_t opt{};
    if (keyframes != nullptr) {
        kf = BuildCapiParamValue(*keyframes, arena);
    }
    if (options != nullptr) {
        opt = BuildCapiParamValue(*options, arena);
    }
    int32_t code = whisker_lynx_capi()->element_animate(
        element->shell, element->handle, operation, animation_name,
        keyframes != nullptr ? &kf : nullptr,
        options != nullptr ? &opt : nullptr);
    if (code != 0) {
        return MakeBridgeErrorValue(
            ("lynx_element_animate returned non-zero (code=" +
             std::to_string(code) + ")").c_str());
    }
    WhiskerValueRaw ok;
    std::memset(&ok, 0, sizeof(ok));
    ok.type = WHISKER_VALUE_NULL;
    return ok;
}

extern "C" WhiskerValueRaw whisker_bridge_invoke_element_method_with_params(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* params) {
    if (element == nullptr || element->handle == nullptr ||
        element->shell == nullptr || method_name == nullptr) {
        return MakeBridgeErrorValue(
            "whisker_bridge_invoke_element_method_with_params: NULL element / "
            "shell / method");
    }
    int32_t sign = whisker_lynx_capi()->element_id(element->handle);
    if (sign <= 0) {
        return MakeBridgeErrorValue(
            "whisker_bridge_invoke_element_method_with_params: element has no "
            "sign yet (was it flushed into the tree?)");
    }

    CapiArena arena;
    lynx_ui_method_value_t root;
    std::memset(&root, 0, sizeof(root));
    if (params != nullptr) {
        root = BuildCapiParamValue(*params, arena);
    }

    int32_t code = whisker_lynx_capi()->ui_invoke_method_with_params(
        element->shell, sign, method_name, params != nullptr ? &root : nullptr);
    if (code != 0) {
        return MakeBridgeErrorValue(
            ("lynx_ui_invoke_method_with_params returned non-zero (code=" +
             std::to_string(code) + ")").c_str());
    }
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

void element_method_async_adapter(int32_t code,
                                  const lynx_ui_method_value_t* capi_result,
                                  void* user_data) {
    auto* ctx = static_cast<ElementMethodAsyncCtx*>(user_data);
    if (ctx == nullptr) return;
    if (ctx->rust_cb != nullptr) {
        WhiskerValueRaw result;
        if (code != 0) {
            // Non-SUCCESS UI-method code (UNKNOWN=1, PARAM_INVALID=4,
            // NO_UI_FOR_NODE=6, …). The result data on the error path is
            // often null, so surface a real error instead of letting it
            // deserialize into a misleading "invalid type: null". Append
            // the platform's message when it rides along as a string.
            std::string msg = "UI method failed (code " + std::to_string(code) + ")";
            if (capi_result != nullptr &&
                capi_result->type == LYNX_UI_METHOD_VALUE_STRING &&
                capi_result->v.s != nullptr) {
                msg += ": ";
                msg += capi_result->v.s;
            }
            result = MakeBridgeErrorValue(msg.c_str());
        } else {
            result = CapiValueToWhisker(capi_result);
        }
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
    int32_t sign = whisker_lynx_capi()->element_id(element->handle);
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
    int32_t code = whisker_lynx_capi()->ui_invoke_method_async(
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

// The unified element-method path: a single `params` value (a
// `WHISKER_VALUE_MAP`, passed through as the params object directly) +
// an async result callback. Built on `lynx_ui_invoke_method_async_with_params`.
// Reuses the `CapiArena` recursive converter (shared with the
// fire-and-forget params path) and the async result adapter. The arena
// only needs to outlive the call: the capi converts the value_t tree
// into lepus synchronously before scheduling, and the result callback
// reads only that lepus copy.
extern "C" bool whisker_bridge_invoke_element_method_async_with_params(
    WhiskerElement* element,
    const char* method_name,
    const WhiskerValueRaw* params,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (element == nullptr || element->handle == nullptr ||
        element->shell == nullptr || method_name == nullptr) {
        FailElementMethodAsync(callback, user_data,
                               "whisker_bridge_invoke_element_method_async_with_params: "
                               "NULL element / shell / method");
        return false;
    }
    int32_t sign = whisker_lynx_capi()->element_id(element->handle);
    if (sign <= 0) {
        FailElementMethodAsync(callback, user_data,
                               "whisker_bridge_invoke_element_method_async_with_params: "
                               "element has no sign yet");
        return false;
    }

    CapiArena arena;
    lynx_ui_method_value_t root;
    std::memset(&root, 0, sizeof(root));
    if (params != nullptr) {
        root = BuildCapiParamValue(*params, arena);
    }
    auto* ctx = new ElementMethodAsyncCtx{callback, user_data};
    int32_t code = whisker_lynx_capi()->ui_invoke_method_async_with_params(
        element->shell, sign, method_name, params != nullptr ? &root : nullptr,
        element_method_async_adapter, ctx);
    if (code != 0) {
        delete ctx;
        FailElementMethodAsync(
            callback, user_data,
            ("lynx_ui_invoke_method_async_with_params returned non-zero (code=" +
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
