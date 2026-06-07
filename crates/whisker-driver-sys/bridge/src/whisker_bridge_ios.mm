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
#import <objc/runtime.h>

// Step-6 build decoupling: vendored stub @interface declarations for
// the Lynx Obj-C types this file touches. Replaces every
// `#import <Lynx/...>` so the bridge .mm compiles without `-F` paths
// into the staged Lynx xcframework. At link time, the only Lynx-side
// thing left is the `objc_getClass("LynxTouchEvent")` lookup below —
// that's a C string, not an `_OBJC_CLASS_$_` symbol.
#include "lynx_objc_stubs.h"

#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "whisker_bridge.h"
#include "whisker_bridge_internal.h"

namespace {

// ---- NSObject → WhiskerValueRaw marshalling -------------------------------
//
// The event reporter hands us the event body as an NSDictionary
// (`[LynxEvent generateEventBody]`). We lower it into the same
// `WhiskerValueRaw` tagged-union wire module args/returns use, so the
// Rust side decodes events through one path (`from_raw`) with no JSON
// round-trip. The tree is heap-owned (malloc); the public
// `whisker_bridge_value_release` (recursive child-free, leaves the
// stack-owned top struct) releases it after dispatch.

WhiskerValueRaw WhiskerValueFromNSObject(id obj);

WhiskerStringRef MakeStringRef(NSString* s) {
    WhiskerStringRef ref{nullptr, 0};
    if (s == nil) return ref;
    const char* utf8 = [s UTF8String];
    if (utf8 == nullptr) return ref;
    size_t len = std::strlen(utf8);
    char* buf = static_cast<char*>(std::malloc(len == 0 ? 1 : len));
    if (len > 0) std::memcpy(buf, utf8, len);
    ref.ptr = buf;
    ref.len = len;
    return ref;
}

WhiskerValueRaw WhiskerValueFromNSObject(id obj) {
    WhiskerValueRaw v;
    std::memset(&v, 0, sizeof(v));
    if (obj == nil || [obj isKindOfClass:[NSNull class]]) {
        v.type = WHISKER_VALUE_NULL;
        return v;
    }
    if ([obj isKindOfClass:[NSString class]]) {
        v.type = WHISKER_VALUE_STRING;
        v.v.s = MakeStringRef(static_cast<NSString*>(obj));
        return v;
    }
    if ([obj isKindOfClass:[NSNumber class]]) {
        NSNumber* n = static_cast<NSNumber*>(obj);
        // `__NSCFBoolean` reports as the CFBoolean type — distinguish
        // it so JS-style booleans don't become Int 0/1.
        if (CFGetTypeID((__bridge CFTypeRef)n) == CFBooleanGetTypeID()) {
            v.type = WHISKER_VALUE_BOOL;
            v.v.b = [n boolValue];
            return v;
        }
        const char* t = [n objCType];
        if (t != nullptr && (t[0] == 'f' || t[0] == 'd')) {
            v.type = WHISKER_VALUE_FLOAT;
            v.v.f = [n doubleValue];
        } else {
            v.type = WHISKER_VALUE_INT;
            v.v.i = [n longLongValue];
        }
        return v;
    }
    if ([obj isKindOfClass:[NSArray class]]) {
        NSArray* arr = static_cast<NSArray*>(obj);
        size_t count = static_cast<size_t>(arr.count);
        v.type = WHISKER_VALUE_ARRAY;
        v.v.array.count = count;
        v.v.array.items = count > 0
            ? static_cast<WhiskerValueRaw*>(std::malloc(sizeof(WhiskerValueRaw) * count))
            : nullptr;
        for (size_t i = 0; i < count; i++) {
            v.v.array.items[i] = WhiskerValueFromNSObject(arr[static_cast<NSUInteger>(i)]);
        }
        return v;
    }
    if ([obj isKindOfClass:[NSDictionary class]]) {
        NSDictionary* dict = static_cast<NSDictionary*>(obj);
        size_t count = static_cast<size_t>(dict.count);
        v.type = WHISKER_VALUE_MAP;
        v.v.map.count = count;
        v.v.map.entries = count > 0
            ? static_cast<WhiskerKeyValueRaw*>(std::malloc(sizeof(WhiskerKeyValueRaw) * count))
            : nullptr;
        size_t i = 0;
        for (id key in dict) {
            if (i >= count) break;
            NSString* ks = [key isKindOfClass:[NSString class]]
                ? static_cast<NSString*>(key)
                : [key description];
            v.v.map.entries[i].key = MakeStringRef(ks);
            v.v.map.entries[i].value = WhiskerValueFromNSObject(dict[key]);
            i++;
        }
        return v;
    }
    v.type = WHISKER_VALUE_NULL;
    return v;
}

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
        // `generateEventBody` is the public seam on `LynxEvent` (the
        // base class), returning the dict JS-side handlers would have
        // received. For custom events (input / change / etc.) it
        // carries the user-supplied params. Marshal it straight into
        // a `WhiskerValueRaw` tree (no JSON) and hand the bridge a
        // pointer; release the heap-owned tree after dispatch.
        //
        // BUT: `LynxTouchEvent` does NOT override `generateEventBody`
        // — only `getEventParams` carries `clientPoint` / `pagePoint`
        // / `viewPoint`. The body that the base class hands us is the
        // bare `{type, target, currentTarget}` dict, so touch events
        // arriving via this path lose their coordinates. We splice
        // the touches/changedTouches/detail entries onto the body
        // here, mirroring the shape the JS-side `event.touches[i]`
        // surface would expose.
        WhiskerValueRaw value;
        bool have_value = false;
        @try {
            NSMutableDictionary* body = [event generateEventBody];
            if (body != nil) {
                // Step-6: avoid emitting an `_OBJC_CLASS_$_LynxTouchEvent`
                // link-time reference. The class is registered with the
                // Obj-C runtime by Lynx.framework at dyld load time, so a
                // name-based lookup resolves to the same class object
                // `[LynxTouchEvent class]` would have returned — but
                // through a C string, not a class symbol. The downcast
                // below is still typed against our stub @interface so
                // the property accessors compile to the right
                // objc_msgSend selectors.
                Class lynxTouchEventClass = objc_getClass("LynxTouchEvent");
                if (lynxTouchEventClass != Nil &&
                    [event isKindOfClass:lynxTouchEventClass]) {
                    LynxTouchEvent* touch = (LynxTouchEvent*)event;
                    if (!touch.isMultiTouch) {
                        // Single-touch shape — one synthesized
                        // `touches[0]` (identifier 0) carrying every
                        // coordinate space Lynx tracks.
                        NSDictionary* t = @{
                            @"identifier": @(0),
                            @"x": @(touch.pagePoint.x),
                            @"y": @(touch.pagePoint.y),
                            @"pageX": @(touch.pagePoint.x),
                            @"pageY": @(touch.pagePoint.y),
                            @"clientX": @(touch.clientPoint.x),
                            @"clientY": @(touch.clientPoint.y),
                        };
                        body[@"touches"] = @[t];
                        body[@"changedTouches"] = @[t];
                        body[@"detail"] = @{
                            @"x": @(touch.pagePoint.x),
                            @"y": @(touch.pagePoint.y),
                        };
                    } else if (touch.touchMap != nil) {
                        // Multi-touch shape — touchMap is keyed by
                        // touch identifier with values `@[clientX,
                        // clientY, pageX, pageY, viewX, viewY]`.
                        NSMutableArray* touches = [NSMutableArray array];
                        [touch.touchMap enumerateKeysAndObjectsUsingBlock:
                            ^(id key, id obj, BOOL* stop) {
                                if (![obj isKindOfClass:[NSArray class]]) return;
                                NSArray* arr = (NSArray*)obj;
                                if (arr.count < 6) return;
                                NSNumber* identifier = nil;
                                if ([key isKindOfClass:[NSNumber class]]) {
                                    identifier = (NSNumber*)key;
                                } else {
                                    identifier = @([[key description] integerValue]);
                                }
                                [touches addObject:@{
                                    @"identifier": identifier,
                                    @"x": arr[2],
                                    @"y": arr[3],
                                    @"pageX": arr[2],
                                    @"pageY": arr[3],
                                    @"clientX": arr[0],
                                    @"clientY": arr[1],
                                }];
                            }];
                        body[@"touches"] = touches;
                        body[@"changedTouches"] = touches;
                        if (touches.count > 0) {
                            NSDictionary* first = touches.firstObject;
                            body[@"detail"] = @{
                                @"x": first[@"pageX"],
                                @"y": first[@"pageY"],
                            };
                        }
                    }
                }
                value = WhiskerValueFromNSObject(body);
                have_value = true;
            }
        } @catch (NSException* exn) {
            // Swallow — degrade to "no body" rather than crash the
            // event-reporter chain.
            have_value = false;
        }
        bool handled = whisker_bridge_internal_dispatch_event(
            (int32_t)event.targetSign,
            [event.eventName UTF8String],
            have_value ? &value : nullptr);
        if (have_value) {
            whisker_bridge_value_release(&value);
        }
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

// iOS counterpart to the Android `__android_log_print` diag helper.
// Routes through `NSLog` so a "diag value=N" line shows up in
// `xcrun simctl spawn ... log` / Console.app under the given tag.
// Keeps the symbol present even when no Rust caller currently uses
// it, so a `whisker_bridge_debug_log_i32(b"FOO\0".as_ptr(), n)`
// added during a debugging session links on both platforms.
extern "C" void whisker_bridge_debug_log_i32(const char* tag, int32_t value) {
    NSLog(@"[%s] diag value=%d", tag != nullptr ? tag : "WHISKER", value);
}


#endif  // __APPLE__
