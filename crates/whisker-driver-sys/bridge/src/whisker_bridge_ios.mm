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
#include <string>

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
        // `generateEventBody` is the public seam on `LynxEvent` (the
        // base class), returning the dict that JS-side handlers would
        // have received. For touch events the dict has the canonical
        // touch points; for custom events (input / change / etc.) it
        // contains the user-supplied params. Serialise via
        // `NSJSONSerialization` so the bridge can hand it to the Rust
        // callback as a UTF-8 string.
        const char* payload_c = "";
        std::string payload_storage;
        @try {
            NSMutableDictionary* body = [event generateEventBody];
            if (body != nil &&
                [NSJSONSerialization isValidJSONObject:body]) {
                NSError* err = nil;
                NSData* data = [NSJSONSerialization dataWithJSONObject:body
                                                              options:0
                                                                error:&err];
                if (data != nil && err == nil) {
                    payload_storage.assign(
                        (const char*)data.bytes, (size_t)data.length);
                    payload_c = payload_storage.c_str();
                }
            }
        } @catch (NSException* exn) {
            // Swallow — degrade to empty payload rather than crash
            // the event-reporter chain.
        }
        bool handled = whisker_bridge_internal_dispatch_event(
            (int32_t)event.targetSign,
            [event.eventName UTF8String],
            payload_c);
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

// ---- Native module invocation (Phase 7-Φ.E.2) ----------------------------
//
// Replaces the common-side stub (which always returned
// WHISKER_VALUE_ERROR) with the real iOS dispatch: look up the class
// via WhiskerModuleRegistry, build an NSInvocation against the
// single-`NSArray*`-arg selector, convert WhiskerValue ↔ Foundation
// types, invoke, return the converted result.
//
// Method convention: each Whisker module method is declared with a
// single `NSArray*` argument and an `id` return type. The Obj-C
// selector is `<methodName>:` (the leading word + one colon). The
// `NSArray*` carries the positional args after WhiskerValue →
// Foundation conversion. Author-facing types are recovered by the
// `#[whisker::native_module]` proc-macro-generated proxy + the
// `@WhiskerMethod` Swift Macro on the platform side (both arrive in
// later sub-phases).

#import "whisker_module_registry.h"
#import <objc/runtime.h>

namespace {

// `MakeErrorValue` lives in whisker_bridge_common.cc as a static
// — duplicate it here so the iOS bridge unit doesn't depend on
// the common stubs once they're removed in a follow-up.
WhiskerValue WhiskerMakeErrorValue(const char* message) {
    WhiskerValue v;
    std::memset(&v, 0, sizeof(v));
    v.type = WHISKER_VALUE_ERROR;
    if (message != nullptr) {
        // Strings returned from invoke_module are heap-owned by the
        // bridge — caller frees via whisker_bridge_value_release.
        size_t len = std::strlen(message);
        char* buf = (char*)std::malloc(len + 1);
        std::memcpy(buf, message, len + 1);
        v.v.s.ptr = buf;
        v.v.s.len = len;
    }
    return v;
}

// Convert a single `WhiskerValue` (borrowed from the caller) into
// a Foundation object suitable for stuffing into an `NSArray`.
// Returns `NSNull` for the null variant; the array-variant
// recursively converts each element; the map-variant builds an
// `NSDictionary` keyed by UTF-8 string.
id WhiskerValueToFoundation(const WhiskerValue* v) {
    if (v == nullptr) return [NSNull null];
    switch (v->type) {
        case WHISKER_VALUE_NULL:
            return [NSNull null];
        case WHISKER_VALUE_BOOL:
            return @(v->v.b);
        case WHISKER_VALUE_INT:
            return @(v->v.i);
        case WHISKER_VALUE_FLOAT:
            return @(v->v.f);
        case WHISKER_VALUE_STRING: {
            if (v->v.s.ptr == nullptr) return @"";
            return [[NSString alloc] initWithBytes:v->v.s.ptr
                                            length:v->v.s.len
                                          encoding:NSUTF8StringEncoding] ?: @"";
        }
        case WHISKER_VALUE_BYTES: {
            if (v->v.bytes.ptr == nullptr || v->v.bytes.len == 0) {
                return [NSData data];
            }
            return [NSData dataWithBytes:v->v.bytes.ptr length:v->v.bytes.len];
        }
        case WHISKER_VALUE_ARRAY: {
            NSMutableArray* arr = [NSMutableArray arrayWithCapacity:v->v.array.count];
            for (size_t i = 0; i < v->v.array.count; i++) {
                [arr addObject:WhiskerValueToFoundation(&v->v.array.items[i])];
            }
            return arr;
        }
        case WHISKER_VALUE_MAP: {
            NSMutableDictionary* dict =
                [NSMutableDictionary dictionaryWithCapacity:v->v.map.count];
            for (size_t i = 0; i < v->v.map.count; i++) {
                NSString* key = nil;
                if (v->v.map.entries[i].key.ptr != nullptr) {
                    key = [[NSString alloc]
                        initWithBytes:v->v.map.entries[i].key.ptr
                               length:v->v.map.entries[i].key.len
                             encoding:NSUTF8StringEncoding];
                }
                if (key == nil) continue;
                id value = WhiskerValueToFoundation(&v->v.map.entries[i].value);
                dict[key] = value;
            }
            return dict;
        }
        case WHISKER_VALUE_ERROR:
        default:
            return [NSNull null];
    }
}

// Convert a Foundation object back into a `WhiskerValue`. Heap
// allocations attached to the result are owned by the bridge —
// callers free via whisker_bridge_value_release once they've
// copied any inner data they need.
WhiskerValue FoundationToWhiskerValue(id obj) {
    WhiskerValue v;
    std::memset(&v, 0, sizeof(v));
    if (obj == nil || obj == [NSNull null]) {
        v.type = WHISKER_VALUE_NULL;
        return v;
    }
    if ([obj isKindOfClass:[NSNumber class]]) {
        NSNumber* n = obj;
        // `objCType` distinguishes the underlying scalar — `c` /
        // `B` for BOOL, `q`/`l`/`i`/... for integers, `d`/`f` for
        // floats. Bool is a special case in Obj-C: `@YES`'s
        // `objCType` is `c` but `kCFBooleanTrue` is the canonical
        // singleton, so we compare instance identity to handle it
        // reliably.
        if ((__bridge CFBooleanRef)obj == kCFBooleanTrue ||
            (__bridge CFBooleanRef)obj == kCFBooleanFalse) {
            v.type = WHISKER_VALUE_BOOL;
            v.v.b = [n boolValue];
            return v;
        }
        const char* t = [n objCType];
        if (t != nullptr &&
            (*t == 'd' || *t == 'f')) {
            v.type = WHISKER_VALUE_FLOAT;
            v.v.f = [n doubleValue];
            return v;
        }
        v.type = WHISKER_VALUE_INT;
        v.v.i = [n longLongValue];
        return v;
    }
    if ([obj isKindOfClass:[NSString class]]) {
        NSString* s = obj;
        const char* utf8 = [s UTF8String];
        size_t len = (utf8 != nullptr) ? std::strlen(utf8) : 0;
        char* buf = (char*)std::malloc(len + 1);
        if (utf8 != nullptr) {
            std::memcpy(buf, utf8, len + 1);
        } else {
            buf[0] = '\0';
        }
        v.type = WHISKER_VALUE_STRING;
        v.v.s.ptr = buf;
        v.v.s.len = len;
        return v;
    }
    if ([obj isKindOfClass:[NSData class]]) {
        NSData* d = obj;
        size_t len = (size_t)d.length;
        uint8_t* buf = (uint8_t*)std::malloc(len);
        if (len > 0) std::memcpy(buf, d.bytes, len);
        v.type = WHISKER_VALUE_BYTES;
        v.v.bytes.ptr = buf;
        v.v.bytes.len = len;
        return v;
    }
    if ([obj isKindOfClass:[NSArray class]]) {
        NSArray* a = obj;
        size_t count = (size_t)a.count;
        WhiskerValue* items =
            (WhiskerValue*)std::malloc(count * sizeof(WhiskerValue));
        for (size_t i = 0; i < count; i++) {
            items[i] = FoundationToWhiskerValue(a[i]);
        }
        v.type = WHISKER_VALUE_ARRAY;
        v.v.array.items = items;
        v.v.array.count = count;
        return v;
    }
    if ([obj isKindOfClass:[NSDictionary class]]) {
        NSDictionary* d = obj;
        size_t count = (size_t)d.count;
        WhiskerKeyValue* entries =
            (WhiskerKeyValue*)std::malloc(count * sizeof(WhiskerKeyValue));
        size_t i = 0;
        for (NSString* key in d.allKeys) {
            const char* utf8 = [key UTF8String];
            size_t key_len = (utf8 != nullptr) ? std::strlen(utf8) : 0;
            char* key_buf = (char*)std::malloc(key_len + 1);
            if (utf8 != nullptr) {
                std::memcpy(key_buf, utf8, key_len + 1);
            } else {
                key_buf[0] = '\0';
            }
            entries[i].key.ptr = key_buf;
            entries[i].key.len = key_len;
            entries[i].value = FoundationToWhiskerValue(d[key]);
            i++;
        }
        v.type = WHISKER_VALUE_MAP;
        v.v.map.entries = entries;
        v.v.map.count = count;
        return v;
    }
    // Unknown / unsupported Foundation type — return null rather
    // than crashing; the proxy on the Rust side reports it as a
    // "type mismatch" if the user asked for a specific shape.
    v.type = WHISKER_VALUE_NULL;
    return v;
}

void WhiskerValueRelease(WhiskerValue* v) {
    if (v == nullptr) return;
    switch (v->type) {
        case WHISKER_VALUE_STRING:
        case WHISKER_VALUE_ERROR:
            std::free((void*)v->v.s.ptr);
            v->v.s.ptr = nullptr;
            v->v.s.len = 0;
            break;
        case WHISKER_VALUE_BYTES:
            std::free((void*)v->v.bytes.ptr);
            v->v.bytes.ptr = nullptr;
            v->v.bytes.len = 0;
            break;
        case WHISKER_VALUE_ARRAY:
            for (size_t i = 0; i < v->v.array.count; i++) {
                WhiskerValueRelease(&v->v.array.items[i]);
            }
            std::free(v->v.array.items);
            v->v.array.items = nullptr;
            v->v.array.count = 0;
            break;
        case WHISKER_VALUE_MAP:
            for (size_t i = 0; i < v->v.map.count; i++) {
                std::free((void*)v->v.map.entries[i].key.ptr);
                WhiskerValueRelease(&v->v.map.entries[i].value);
            }
            std::free(v->v.map.entries);
            v->v.map.entries = nullptr;
            v->v.map.count = 0;
            break;
        default:
            break;
    }
    v->type = WHISKER_VALUE_NULL;
}

}  // namespace

// `__attribute__((used))` so the iOS dispatch overrides the
// common-stub symbol once linked into the bridge static archive
// — the linker's last-definition-wins behavior under `-ObjC`
// pulls our impl in.
__attribute__((used))
extern "C" WhiskerValue whisker_bridge_invoke_module(
    const char* module_name,
    const char* method_name,
    const WhiskerValue* args,
    size_t arg_count) {
    @autoreleasepool {
        if (module_name == nullptr || method_name == nullptr) {
            return WhiskerMakeErrorValue("module/method name is NULL");
        }
        NSString* nameStr = [NSString stringWithUTF8String:module_name];
        id instance = [WhiskerModuleRegistry moduleInstanceForName:nameStr];
        if (instance == nil) {
            return WhiskerMakeErrorValue("module not registered");
        }
        NSString* selStr = [NSString stringWithFormat:@"%s:", method_name];
        SEL selector = NSSelectorFromString(selStr);
        if (![instance respondsToSelector:selector]) {
            return WhiskerMakeErrorValue("module method not found");
        }

        NSMutableArray* argsArr = [NSMutableArray arrayWithCapacity:arg_count];
        for (size_t i = 0; i < arg_count; i++) {
            id obj = WhiskerValueToFoundation(&args[i]);
            [argsArr addObject:(obj ?: [NSNull null])];
        }

        NSMethodSignature* sig =
            [[instance class] instanceMethodSignatureForSelector:selector];
        NSInvocation* inv = [NSInvocation invocationWithMethodSignature:sig];
        [inv setTarget:instance];
        [inv setSelector:selector];
        [inv setArgument:&argsArr atIndex:2];  // 0 = self, 1 = _cmd

        @try {
            [inv invoke];
        } @catch (NSException* e) {
            return WhiskerMakeErrorValue([e.reason UTF8String] ?: "exception");
        }

        // Read the return value as an `id` (Whisker module methods
        // are required to return an object — primitives must be
        // wrapped in NSNumber on the platform side).
        id returnValue = nil;
        if (sig.methodReturnLength == sizeof(id)) {
            __unsafe_unretained id raw = nil;
            [inv getReturnValue:&raw];
            returnValue = raw;
        }
        return FoundationToWhiskerValue(returnValue);
    }
}

__attribute__((used))
extern "C" void whisker_bridge_value_release(WhiskerValue* value) {
    WhiskerValueRelease(value);
}

// Async path: foundation only — dispatches to the sync path on a
// background queue, then calls the callback on the same queue.
// Callers that need main-thread delivery should bounce via
// `dispatch_async(dispatch_get_main_queue(), ...)` inside their
// callback. Real async storage / network ops will land in
// dedicated module implementations (Phase 7-Φ.E.7+).
__attribute__((used))
extern "C" bool whisker_bridge_invoke_module_async(
    const char* module_name,
    const char* method_name,
    const WhiskerValue* args,
    size_t arg_count,
    WhiskerModuleCallback callback,
    void* user_data) {
    if (callback == nullptr) return false;

    // Copy args + names into heap so the dispatch_async block can
    // outlive this stack frame. `WhiskerValueRelease` runs on the
    // bridge side after the callback returns; deep-copy avoids
    // surprising lifetimes if the caller frees the originals.
    std::string mod = module_name ? module_name : "";
    std::string method = method_name ? method_name : "";

    std::vector<WhiskerValue> args_copy(arg_count);
    for (size_t i = 0; i < arg_count; i++) {
        // Note: this copies the tagged union by-value but does
        // *not* deep-copy the heap allocations under string /
        // bytes / array / map. The async-impl follow-up will add
        // a `WhiskerValueDeepCopy` once we have a real consumer
        // for async; for now this is safe iff the caller keeps
        // the args alive until the callback fires. The sync
        // path's semantics (args borrowed for the call's
        // duration only) doesn't apply to async — caller MUST
        // own the storage.
        args_copy[i] = args[i];
    }

    dispatch_async(
        dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
          WhiskerValue result = whisker_bridge_invoke_module(
              mod.c_str(), method.c_str(),
              args_copy.empty() ? nullptr : args_copy.data(),
              args_copy.size());
          callback(user_data, &result);
          WhiskerValueRelease(&result);
        });
    return true;
}

#endif  // __APPLE__
