// lyra_bridge.mm
//
// Phase 4: bridge exposes a generic Element PAPI surface so the Rust
// runtime can build whatever tree it wants from outside Lynx.

#import <Foundation/Foundation.h>
#import <Lynx/LynxView.h>
#import <Lynx/LynxView+Internal.h>
#import <Lynx/LynxTemplateRender.h>
#import <Lynx/LynxTemplateRender+Internal.h>
#import <Lynx/LynxEngineProxy.h>
#import <Lynx/LynxUIOwner.h>
#import <Lynx/LynxEventHandler.h>
#import <Lynx/LynxEventEmitter.h>
#import <Lynx/LynxEvent.h>
#import <objc/runtime.h>

#include <atomic>
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

#include "lyra_bridge.h"

// ----------------------------------------------------------------------------
// Internal types
// ----------------------------------------------------------------------------

namespace {

// LynxTemplateRender's `shell_` is a protected ivar (declared in
// PrivateHeaders/LynxTemplateRender+Protected.h, which transitively
// pulls in too many `""`-style imports for our header search paths).
// Read it directly via the Obj-C runtime instead.
lynx::shell::LynxShell* GetShell(LynxTemplateRender* render) {
    if (render == nil) return nullptr;
    Ivar ivar = class_getInstanceVariable([render class], "shell_");
    if (ivar == nullptr) return nullptr;
    ptrdiff_t offset = ivar_getOffset(ivar);
    auto* base = reinterpret_cast<uint8_t*>((__bridge void*)render);
    auto* unique = reinterpret_cast<std::unique_ptr<lynx::shell::LynxShell>*>(
        base + offset);
    return unique->get();
}

}  // namespace

struct LyraEngine {
    lynx::shell::LynxShell* shell = nullptr;
    lynx::tasm::ElementManager* manager = nullptr;
    fml::RefPtr<lynx::tasm::PageElement> root_page;
    bool fiber_arch_initialized = false;
    bool event_reporter_installed = false;
};

// LyraElement wraps a strong reference to a FiberElement. The C ABI hands
// out raw pointers; the wrapper keeps the underlying element alive via
// fml::RefPtr. lyra_bridge_release_element drops one strong reference.
struct LyraElement {
    fml::RefPtr<lynx::tasm::FiberElement> ref;
};

// Native event listener registry. Lynx dispatches physical touches through
// `Element::SetEventHandler(EventHandler*)` consumed by `TouchEventHandler`
// (whose ultimate target is the JS runtime), not through the EventTarget /
// AddEventListener path — so we can't just hang a `lynx::event::EventListener`
// off a FiberElement and expect taps to fire it.
//
// Instead we hook the iOS-side `LynxEventEmitter`'s `eventReporter` block,
// which is invoked once per LynxTouchEvent *before* the event is forwarded
// to the engine. The block looks up (element_sign, event_name) in this
// registry and fires the C callback if present, returning YES to consume
// the event so it isn't redundantly forwarded to a (non-existent) JS app.
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
    LyraEventCallback callback;
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
// Engine lifecycle
// ----------------------------------------------------------------------------

// Install our hook on the LynxEventEmitter so physical taps land in our
// native callback registry instead of being dropped on the way to a
// non-existent JS handler. Safe to call repeatedly — only installs once
// per engine.
static void InstallEventReporterIfNeeded(LyraEngine* engine, LynxView* view) {
    if (engine == nullptr || engine->event_reporter_installed) return;
    LynxTemplateRender* render = [view templateRender];
    if (render == nil) return;
    LynxUIOwner* owner = [render uiOwner];
    if (owner == nil) return;
    // The Internal category on LynxUIContext (declared in LynxUIOwner.h)
    // exposes the LynxEventHandler / LynxEventEmitter pair we need.
    LynxEventHandler* handler = owner.uiContext.eventHandler;
    if (handler == nil) return;
    LynxEventEmitter* emitter = handler.eventEmitter;
    if (emitter == nil) return;
    [emitter setEventReporterBlock:^BOOL(LynxEvent* event) {
        if (event == nil || event.eventName == nil) return NO;
        EventCallback hit{nullptr, nullptr};
        bool found = false;
        {
            std::lock_guard<std::mutex> lock(RegistryMutex());
            EventKey key{(int32_t)event.targetSign,
                         std::string([event.eventName UTF8String])};
            auto it = Registry().find(key);
            if (it != Registry().end()) {
                hit = it->second;
                found = true;
            }
        }
        if (found && hit.callback) {
            hit.callback(hit.user_data);
            return YES;
        }
        return NO;
    }];
    engine->event_reporter_installed = true;
}

extern "C" LyraEngine* lyra_bridge_engine_attach(void* lynx_view_ptr) {
    if (lynx_view_ptr == nullptr) return nullptr;
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;
    LynxTemplateRender* render = [view templateRender];
    if (render == nil) return nullptr;
    lynx::shell::LynxShell* shell = GetShell(render);
    if (shell == nullptr) return nullptr;

    auto* engine = new LyraEngine();
    engine->shell = shell;
    InstallEventReporterIfNeeded(engine, view);
    return engine;
}

extern "C" void lyra_bridge_engine_release(LyraEngine* engine) {
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

extern "C" bool lyra_bridge_dispatch(LyraEngine* engine,
                                     LyraTasmCallback callback,
                                     void* user_data) {
    if (engine == nullptr || engine->shell == nullptr || callback == nullptr) {
        return false;
    }
    LyraEngine* engine_capture = engine;
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
    lynx::tasm::ElementManager* manager, LyraElementTag tag) {
    using namespace lynx;
    if (manager == nullptr) return nullptr;
    switch (tag) {
        case LyraElementTagPage:
            return manager->CreateFiberPage(base::String("0"), 0);
        case LyraElementTagView:
            return manager->CreateFiberView();
        case LyraElementTagText:
            return manager->CreateFiberText(base::String("text"));
        case LyraElementTagRawText:
            return manager->CreateFiberRawText();
        case LyraElementTagImage:
            // TODO(phase 4+): expose CreateFiberImage with a proper tag.
            return manager->CreateFiberView();
        case LyraElementTagScrollView:
            return manager->CreateFiberScrollView(
                base::String(lynx::tasm::kElementScrollViewTag));
    }
    return nullptr;
}

}  // namespace

extern "C" LyraElement* lyra_bridge_create_element(LyraEngine* engine,
                                                   LyraElementTag tag) {
    if (engine == nullptr || engine->manager == nullptr) return nullptr;
    auto ref = CreateForTag(engine->manager, tag);
    if (!ref) return nullptr;
    return new LyraElement{std::move(ref)};
}

extern "C" void lyra_bridge_release_element(LyraElement* element) {
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

extern "C" void lyra_bridge_set_attribute(LyraElement* element,
                                          const char* key,
                                          const char* value) {
    if (element == nullptr || !element->ref || key == nullptr || value == nullptr) {
        return;
    }
    element->ref->SetAttribute(
        lynx::base::String(key),
        lynx::lepus::Value(lynx::base::String(value)));
}

extern "C" void lyra_bridge_set_inline_styles(LyraElement* element,
                                              const char* css) {
    if (element == nullptr || !element->ref || css == nullptr) return;
    element->ref->SetRawInlineStyles(lynx::base::String(css));
}

extern "C" void lyra_bridge_append_child(LyraElement* parent,
                                         LyraElement* child) {
    if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
        return;
    }
    parent->ref->InsertNode(child->ref);
}

extern "C" void lyra_bridge_remove_child(LyraElement* parent,
                                         LyraElement* child) {
    if (parent == nullptr || child == nullptr || !parent->ref || !child->ref) {
        return;
    }
    parent->ref->RemoveNode(child->ref);
}

extern "C" void lyra_bridge_set_event_listener(LyraElement* element,
                                               const char* event_name,
                                               LyraEventCallback callback,
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

extern "C" void lyra_bridge_set_root(LyraEngine* engine, LyraElement* page) {
    if (engine == nullptr || engine->manager == nullptr ||
        page == nullptr || !page->ref) {
        return;
    }
    auto page_ref = fml::RefPtr<lynx::tasm::PageElement>(
        static_cast<lynx::tasm::PageElement*>(page->ref.get()));
    engine->manager->SetFiberPageElement(page_ref);
    engine->root_page = std::move(page_ref);
}

extern "C" void lyra_bridge_flush(LyraEngine* engine) {
    if (engine == nullptr || engine->manager == nullptr || !engine->root_page) {
        return;
    }
    engine->root_page->FlushActionsAsRoot();
    auto options = std::make_shared<lynx::tasm::PipelineOptions>();
    engine->manager->OnPatchFinish(options, engine->root_page.get());
}

// ----------------------------------------------------------------------------
// Phase 0–3 leftover (kept so existing examples keep compiling)
// ----------------------------------------------------------------------------

extern "C" void lyra_bridge_log_hello(void) {
    NSLog(@"[LyraBridge] Hello from the Obj-C++ bridge");
}
