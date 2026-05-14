// whisker_bridge_common.cc
//
// Platform-independent bridge implementation.
//
// All `whisker_bridge_*` C-ABI symbols defined here are safe to call from
// any host (iOS, Android, …). Per-platform plumbing (LynxView ivar
// access, event-system hooks) lives in whisker_bridge_ios.mm /
// whisker_bridge_android.cc.

#include <cstdint>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <utility>

#include "core/shell/lynx_shell.h"
#include "core/shell/lynx_engine.h"
#include "core/renderer/template_assembler.h"
#include "core/renderer/page_proxy.h"
#include "core/renderer/dom/element_manager.h"
#include "core/renderer/dom/fiber/fiber_element.h"
#include "core/renderer/dom/fiber/page_element.h"
#include "core/renderer/dom/fiber/text_element.h"
#include "core/renderer/dom/fiber/raw_text_element.h"
#include "core/renderer/dom/fiber/view_element.h"
#include "core/renderer/dom/fiber/image_element.h"
#include "core/renderer/dom/fiber/scroll_element.h"
#include "core/renderer/utils/base/tasm_constants.h"
#include "core/public/pipeline_option.h"
#include "core/template_bundle/template_codec/binary_decoder/page_config.h"
#include "base/include/value/base_string.h"

#include "whisker_bridge.h"
#include "whisker_bridge_internal.h"

// ----------------------------------------------------------------------------
// Internal types
// ----------------------------------------------------------------------------

struct WhiskerEngine {
    lynx::shell::LynxShell* shell = nullptr;
    lynx::tasm::ElementManager* manager = nullptr;
    fml::RefPtr<lynx::tasm::PageElement> root_page;
    bool fiber_arch_initialized = false;
    // Set by the platform glue once it has wired its event hook into the
    // host (LynxEventEmitter on iOS, … on Android). Only meaningful to
    // the glue layer; common code just stores it.
    bool event_reporter_installed = false;
};

// WhiskerElement wraps a strong reference to a FiberElement. The C ABI hands
// out raw pointers; the wrapper keeps the underlying element alive via
// fml::RefPtr. whisker_bridge_release_element drops one strong reference.
struct WhiskerElement {
    fml::RefPtr<lynx::tasm::FiberElement> ref;
};

// Native event listener registry. Lynx dispatches physical touches through
// `Element::SetEventHandler(EventHandler*)` consumed by `TouchEventHandler`
// (whose ultimate target is the JS runtime), not through the EventTarget /
// AddEventListener path — so we can't just hang a `lynx::event::EventListener`
// off a FiberElement and expect taps to fire it.
//
// Instead, each platform's glue installs a "reporter" hook on the host's
// event emitter (LynxEventEmitter on iOS). The hook calls
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

WhiskerEngine* whisker_bridge_internal_engine_create(lynx::shell::LynxShell* shell) {
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
// whisker_bridge_ios.mm / whisker_bridge_android.cc); the common code only
// provides the `release` half.

extern "C" void whisker_bridge_engine_release(WhiskerEngine* engine) {
    if (engine == nullptr) return;
    // The PageElement keeps a back-pointer into ElementManager, which is
    // owned by the shell. Drop the page first to avoid dangling refs if
    // the caller has already torn down the LynxView.
    engine->root_page = nullptr;
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
    WhiskerEngine* engine_capture = engine;
    engine->shell->RunOnTasmThread([engine_capture, callback, user_data]() {
        // Lazy-initialize the fiber architecture + element manager on
        // first dispatch. The TemplateAssembler / ElementManager only
        // exist once the shell is fully constructed, which the shell
        // guarantees by the time RunOnTasmThread invokes us.
        if (!engine_capture->fiber_arch_initialized) {
            auto* tasm = engine_capture->shell->GetTasm();
            if (tasm) {
                auto config = std::make_shared<lynx::tasm::PageConfig>();
                config->SetEnableFiberArch(true);
                tasm->SetPageConfig(config);

                auto* page_proxy = tasm->page_proxy();
                if (page_proxy) {
                    engine_capture->manager = page_proxy->element_manager().get();
                }
            }
            engine_capture->fiber_arch_initialized = true;
        }
        callback(user_data);
    });
    return true;
}

// ----------------------------------------------------------------------------
// Element creation / lifetime
// ----------------------------------------------------------------------------

namespace {

fml::RefPtr<lynx::tasm::FiberElement> CreateForTag(
    lynx::tasm::ElementManager* manager, WhiskerElementTag tag) {
    using namespace lynx;
    if (manager == nullptr) return nullptr;
    switch (tag) {
        case WhiskerElementTagPage:
            return manager->CreateFiberPage(base::String("0"), 0);
        case WhiskerElementTagView:
            return manager->CreateFiberView();
        case WhiskerElementTagText:
            return manager->CreateFiberText(base::String("text"));
        case WhiskerElementTagRawText:
            return manager->CreateFiberRawText();
        case WhiskerElementTagImage:
            // TODO(phase 4+): expose CreateFiberImage with a proper tag.
            return manager->CreateFiberView();
        case WhiskerElementTagScrollView:
            return manager->CreateFiberScrollView(
                base::String(lynx::tasm::kElementScrollViewTag));
    }
    return nullptr;
}

}  // namespace

extern "C" WhiskerElement* whisker_bridge_create_element(WhiskerEngine* engine,
                                                   WhiskerElementTag tag) {
    if (engine == nullptr || engine->manager == nullptr) return nullptr;
    auto ref = CreateForTag(engine->manager, tag);
    if (!ref) return nullptr;
    return new WhiskerElement{std::move(ref)};
}

extern "C" void whisker_bridge_release_element(WhiskerElement* element) {
    if (element == nullptr) return;
    // Drop any registered native event callbacks for this element so its
    // sign can't accidentally collide with a future element's id.
    if (element->ref) {
        int32_t sign = element->ref->impl_id();
        std::lock_guard<std::mutex> lock(RegistryMutex());
        for (auto it = Registry().begin(); it != Registry().end(); ) {
            if (it->first.element_sign == sign) {
                it = Registry().erase(it);
            } else {
                ++it;
            }
        }
    }
    delete element;
}

// ----------------------------------------------------------------------------
// Element manipulation
// ----------------------------------------------------------------------------

extern "C" void whisker_bridge_set_attribute(WhiskerElement* element,
                                          const char* key,
                                          const char* value) {
    if (element == nullptr || !element->ref || key == nullptr || value == nullptr) {
        return;
    }
    element->ref->SetAttribute(
        lynx::base::String(key),
        lynx::lepus::Value(lynx::base::String(value)));
}

extern "C" void whisker_bridge_set_inline_styles(WhiskerElement* element,
                                              const char* css) {
    if (element == nullptr || !element->ref || css == nullptr) return;
    element->ref->SetRawInlineStyles(lynx::base::String(css));
}

extern "C" void whisker_bridge_append_child(WhiskerElement* parent,
                                         WhiskerElement* child) {
    if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
        return;
    }
    parent->ref->InsertNode(child->ref);
}

extern "C" void whisker_bridge_remove_child(WhiskerElement* parent,
                                         WhiskerElement* child) {
    if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
        return;
    }
    parent->ref->RemoveNode(child->ref);
}

extern "C" void whisker_bridge_set_event_listener(WhiskerElement* element,
                                               const char* event_name,
                                               WhiskerEventCallback callback,
                                               void* user_data) {
    if (element == nullptr || !element->ref || event_name == nullptr ||
        callback == nullptr) {
        return;
    }
    EventKey key{element->ref->impl_id(), std::string(event_name)};
    std::lock_guard<std::mutex> lock(RegistryMutex());
    Registry()[key] = EventCallback{callback, user_data};
}

// ----------------------------------------------------------------------------
// Pipeline
// ----------------------------------------------------------------------------

extern "C" void whisker_bridge_set_root(WhiskerEngine* engine, WhiskerElement* page) {
    if (engine == nullptr || engine->manager == nullptr ||
        page == nullptr || !page->ref) {
        return;
    }
    auto page_ref = fml::RefPtr<lynx::tasm::PageElement>(
        static_cast<lynx::tasm::PageElement*>(page->ref.get()));
    engine->manager->SetFiberPageElement(page_ref);
    engine->root_page = std::move(page_ref);
}

extern "C" void whisker_bridge_flush(WhiskerEngine* engine) {
    if (engine == nullptr || engine->manager == nullptr || !engine->root_page) {
        return;
    }
    engine->root_page->FlushActionsAsRoot();
    auto options = std::make_shared<lynx::tasm::PipelineOptions>();
    engine->manager->OnPatchFinish(options, engine->root_page.get());
}
