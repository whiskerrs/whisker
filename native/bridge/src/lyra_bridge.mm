// lyra_bridge.mm
//
// Phase 3c: drive Element PAPI directly from the bridge so we can render
// Lynx-managed widgets without ever loading a `.lynx` template.

#import <Foundation/Foundation.h>
#import <Lynx/LynxView.h>
#import <Lynx/LynxView+Internal.h>
#import <Lynx/LynxTemplateRender.h>
#import <Lynx/LynxTemplateRender+Internal.h>
#import <Lynx/LynxEngineProxy.h>
#import <objc/runtime.h>

#include <memory>
#include <string>

// Lynx C++ headers — staged into target/lynx-ios/sources/{Lynx,…}/ by
// scripts/build-lynx-xcframeworks.sh. Header search paths configured in
// Package.swift's LyraBridge cxxSettings.
#include "core/shell/lynx_shell.h"
#include "core/shell/lynx_engine.h"
#include "core/renderer/template_assembler.h"
#include "core/renderer/page_proxy.h"
#include "core/renderer/dom/element_manager.h"
#include "core/renderer/dom/fiber/fiber_element.h"
#include "core/renderer/dom/fiber/page_element.h"
#include "core/renderer/dom/fiber/text_element.h"
#include "core/renderer/dom/fiber/raw_text_element.h"
#include "core/public/pipeline_option.h"
#include "base/include/value/base_string.h"

namespace {

// LynxTemplateRender keeps its `lynx::shell::LynxShell` as a
// `std::unique_ptr<…>` ivar named `shell_`. The accessor is declared in
// PrivateHeaders/LynxTemplateRender+Protected.h, but importing that
// header drags in dozens of Lynx Obj-C headers whose `""`-style includes
// the bridge's include search paths can't resolve. Use the Obj-C runtime
// to find the ivar offset instead, then read the unique_ptr in place.
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

extern "C" {

void lyra_bridge_log_hello(void) {
    NSLog(@"[LyraBridge] Hello from the Obj-C++ bridge");
}

bool lyra_bridge_dispatch_log(void* lynx_view_ptr) {
    if (lynx_view_ptr == nullptr) return false;
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;
    LynxTemplateRender* render = [view templateRender];
    if (render == nil) return false;
    LynxEngineProxy* proxy = [render getEngineProxy];
    if (proxy == nil) return false;
    NSLog(@"[LyraBridge] Dispatching task onto the Lynx TASM thread …");
    [proxy dispatchTaskToLynxEngine:^{
        NSLog(@"[LyraBridge] Hello from the Lynx TASM thread!");
    }];
    return true;
}

bool lyra_bridge_render_text(void* lynx_view_ptr, const char* text) {
    if (lynx_view_ptr == nullptr || text == nullptr) return false;
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;
    LynxTemplateRender* render = [view templateRender];
    lynx::shell::LynxShell* shell = GetShell(render);
    if (shell == nullptr) {
        NSLog(@"[LyraBridge] LynxShell is null — cannot drive Element PAPI");
        return false;
    }

    std::string text_copy(text);

    shell->RunOnTasmThread([shell, text_copy = std::move(text_copy)]() {
        auto* tasm = shell->GetTasm();
        if (tasm == nullptr) return;
        auto* page_proxy = tasm->page_proxy();
        if (page_proxy == nullptr) return;
        auto& manager = page_proxy->element_manager();
        if (!manager) {
            NSLog(@"[LyraBridge] ElementManager is null");
            return;
        }

        using namespace lynx::tasm;

        // Build a minimal element tree:  <page> <text> <raw-text/> </text> </page>
        auto page = manager->CreateFiberPage(lynx::base::String("0"), 0);
        manager->SetFiberPageElement(page);

        auto text_node = manager->CreateFiberText(lynx::base::String("text"));
        auto raw_text  = manager->CreateFiberRawText();

        // Attach the actual text content as the raw-text element's
        // `text` attribute. lepus::Value(string) constructs a string-typed
        // value Lynx understands.
        raw_text->SetAttribute(
            lynx::base::String("text"),
            lynx::lepus::Value(lynx::base::String(text_copy)));

        text_node->InsertNode(raw_text);
        page->InsertNode(text_node);

        auto options = std::make_shared<lynx::tasm::PipelineOptions>();
        manager->OnPatchFinish(options, page.get());

        NSLog(@"[LyraBridge] Element PAPI tree submitted (text=\"%s\")",
              text_copy.c_str());
    });
    return true;
}

}  // extern "C"
