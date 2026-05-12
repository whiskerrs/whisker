// lyra_bridge.mm
//
// Phase 3b: walks LynxView → templateRender → engineProxy and dispatches
// a task onto the Lynx TASM thread. We don't touch Element PAPI yet —
// that's Phase 3c.

#import <Foundation/Foundation.h>
#import <Lynx/LynxView.h>
#import <Lynx/LynxView+Internal.h>
#import <Lynx/LynxTemplateRender.h>
#import <Lynx/LynxTemplateRender+Internal.h>
#import <Lynx/LynxEngineProxy.h>

#import "lyra_bridge.h"

extern "C" {

void lyra_bridge_log_hello(void) {
    NSLog(@"[LyraBridge] Hello from the Obj-C++ bridge");
}

bool lyra_bridge_dispatch_log(void* lynx_view_ptr) {
    if (lynx_view_ptr == nullptr) {
        NSLog(@"[LyraBridge] dispatch_log called with null view");
        return false;
    }

    // The opaque pointer is an `id`-style retained reference owned by the
    // Swift side. Bridge into an Obj-C object reference WITHOUT changing
    // the retain count.
    LynxView* view = (__bridge LynxView*)lynx_view_ptr;

    LynxTemplateRender* render = [view templateRender];
    if (render == nil) {
        NSLog(@"[LyraBridge] view.templateRender is nil");
        return false;
    }

    LynxEngineProxy* proxy = [render getEngineProxy];
    if (proxy == nil) {
        NSLog(@"[LyraBridge] templateRender.engineProxy is nil");
        return false;
    }

    NSLog(@"[LyraBridge] Dispatching task onto the Lynx TASM thread …");
    [proxy dispatchTaskToLynxEngine:^{
        NSLog(@"[LyraBridge] Hello from the Lynx TASM thread!");
    }];

    return true;
}

}  // extern "C"
