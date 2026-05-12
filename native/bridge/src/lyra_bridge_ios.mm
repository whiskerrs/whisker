// lyra_bridge_ios.mm
//
// iOS-specific glue: extracts the LynxShell from a LynxView and installs
// the LynxEventEmitter eventReporter block. All actual Element PAPI work
// happens in lyra_bridge_common.cc.

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

#include <memory>

#include "core/shell/lynx_shell.h"

#include "lyra_bridge.h"
#include "lyra_bridge_internal.h"

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

// Install our hook on the LynxEventEmitter so physical taps land in our
// native callback registry instead of being dropped on the way to a
// non-existent JS handler. Safe to call repeatedly — only installs once
// per engine.
void InstallEventReporterIfNeeded(LyraEngine* engine, LynxView* view) {
    if (engine == nullptr ||
        lyra_bridge_internal_is_event_reporter_installed(engine)) {
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
        bool handled = lyra_bridge_internal_dispatch_event(
            (int32_t)event.targetSign,
            [event.eventName UTF8String]);
        return handled ? YES : NO;
    }];
    lyra_bridge_internal_mark_event_reporter_installed(engine);
}

}  // namespace

extern "C" LyraEngine* lyra_bridge_engine_attach(void* lynx_view_ptr) {
    if (lynx_view_ptr == nullptr) return nullptr;
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;
    LynxTemplateRender* render = [view templateRender];
    if (render == nil) return nullptr;
    lynx::shell::LynxShell* shell = GetShell(render);
    if (shell == nullptr) return nullptr;

    LyraEngine* engine = lyra_bridge_internal_engine_create(shell);
    InstallEventReporterIfNeeded(engine, view);
    return engine;
}

// Kept so older Phase 0–3 examples that still poke this symbol compile.
extern "C" void lyra_bridge_log_hello(void) {
    NSLog(@"[LyraBridge] Hello from the Obj-C++ bridge");
}
