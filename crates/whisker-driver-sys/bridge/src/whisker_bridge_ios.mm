// whisker_bridge_ios.mm
//
// iOS-specific glue: extracts the LynxShell from a LynxView and installs
// the LynxEventEmitter eventReporter block. All actual Element PAPI work
// happens in whisker_bridge_common.cc.
//
// Gated on `__APPLE__` for the same defense-in-depth reason as
// `whisker_bridge_android.cc`: any build system that scans the bridge
// directory whole gets an empty TU on Android / Linux instead of a
// `<Foundation/Foundation.h> not found` failure.

#if defined(__APPLE__)

#import <Foundation/Foundation.h>
#import <Lynx/LynxView.h>
#import <Lynx/LynxView+Internal.h>
#import <Lynx/LynxTemplateRender.h>
#import <Lynx/LynxTemplateRender+Internal.h>
#import <Lynx/LynxUIOwner.h>
#import <Lynx/LynxEventHandler.h>
#import <Lynx/LynxEventEmitter.h>
#import <Lynx/LynxEvent.h>
#import <objc/runtime.h>

#include <cstdint>

#include "whisker_bridge.h"
#include "whisker_bridge_internal.h"

namespace {

// LynxTemplateRender's `shell_` is a protected ivar of static type
// `std::unique_ptr<lynx::shell::LynxShell>`. We can't `#include` Lynx
// C++ headers any more (the new C ABI is the only thing the bridge
// imports from Lynx) — but `std::unique_ptr<T>` with the default
// deleter is a single-pointer-sized member, so reading the ivar's
// raw storage as `void* const*` and dereferencing yields the same
// LynxShell* the C++ code would. We then hand that void* straight to
// `lynx_shell_from_native_ptr` on the Lynx side.
void* GetShellPtr(LynxTemplateRender* render) {
    if (render == nil) return nullptr;
    Ivar ivar = class_getInstanceVariable([render class], "shell_");
    if (ivar == nullptr) return nullptr;
    ptrdiff_t offset = ivar_getOffset(ivar);
    auto* base = reinterpret_cast<uint8_t*>((__bridge void*)render);
    return *reinterpret_cast<void* const*>(base + offset);
}

// Install our hook on the LynxEventEmitter so physical taps land in our
// native callback registry instead of being dropped on the way to a
// non-existent JS handler. Safe to call repeatedly — only installs once
// per engine.
void InstallEventReporterIfNeeded(WhiskerEngine* engine, LynxView* view) {
    if (engine == nullptr ||
        whisker_bridge_internal_is_event_reporter_installed(engine)) {
        return;
    }
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
        bool handled = whisker_bridge_internal_dispatch_event(
            (int32_t)event.targetSign,
            [event.eventName UTF8String]);
        return handled ? YES : NO;
    }];
    whisker_bridge_internal_mark_event_reporter_installed(engine);
}

}  // namespace

extern "C" WhiskerEngine* whisker_bridge_engine_attach(void* lynx_view_ptr) {
    if (lynx_view_ptr == nullptr) return nullptr;
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;
    LynxTemplateRender* render = [view templateRender];
    if (render == nil) return nullptr;
    void* native_shell_ptr = GetShellPtr(render);
    if (native_shell_ptr == nullptr) return nullptr;

    WhiskerEngine* engine = whisker_bridge_internal_engine_create(native_shell_ptr);
    InstallEventReporterIfNeeded(engine, view);
    return engine;
}

// Kept so older Phase 0–3 examples that still poke this symbol compile.
extern "C" void whisker_bridge_log_hello(void) {
    NSLog(@"[WhiskerBridge] Hello from the Obj-C++ bridge");
}

#endif  // __APPLE__
