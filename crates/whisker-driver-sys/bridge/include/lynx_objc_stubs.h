// Minimal vendored @interface stubs for the Lynx Obj-C classes the
// Whisker iOS bridge touches.
//
// Step-6 build decoupling: pre-Step-6 the bridge `#import`'d Lynx's
// public Obj-C headers and the build script added `-F` paths plus
// `-framework Lynx` at link time. That blocked `cargo build` cold —
// the user had to run `whisker build` first to fetch + stage the iOS
// xcframework. Step 6 cuts both ties:
//   * Compile-time: this header replaces every `#import <Lynx/...>`
//     with a stub @interface declaring only the property accessors
//     and method signatures the bridge actually calls. The compiler
//     emits objc_msgSend calls against the named selectors; nothing
//     references `_OBJC_CLASS_$_Lynx*` symbols.
//   * Run-time: `[LynxTouchEvent class]` (the one site that DID emit
//     a class symbol) is replaced with `objc_getClass("LynxTouchEvent")`
//     in the .mm — looks up the real class through the Obj-C runtime
//     against the Lynx.framework that dyld has already mapped via
//     SwiftPM auto-embed.
//
// MUST stay in sync with whiskerrs/lynx's real @interface
// declarations. A renamed selector here means the runtime msgSend
// hits an unrecognised selector at runtime and the app crashes —
// the upside is the failure mode is loud (an NSException dies on the
// event-reporter chain), so misalignment is hard to miss in a smoke
// test.
//
// The class hierarchy used here is the minimum the compiler needs to
// emit calls. LynxView is declared `: UIView` so `(__bridge LynxView*)`
// from a `void*` works as expected; the others are `: NSObject` since
// the bridge only treats them as opaque dispatch targets.

#ifndef WHISKER_BRIDGE_LYNX_OBJC_STUBS_H_
#define WHISKER_BRIDGE_LYNX_OBJC_STUBS_H_

#ifdef __OBJC__

#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#import <CoreGraphics/CGGeometry.h>

NS_ASSUME_NONNULL_BEGIN

@class LynxTemplateRender;
@class LynxUIOwner;
@class LynxUIContext;
@class LynxEventHandler;
@class LynxEventEmitter;
@class LynxEvent;
@class LynxTouchEvent;

// LynxView — UIView subclass, surfaces the underlying template render.
@interface LynxView : UIView
- (nullable LynxTemplateRender*)templateRender;
@end

// LynxTemplateRender — the bridge-visible surface is `uiOwner` plus the
// protected `shell_` ivar that the loader reads via reflection (no
// declared accessor needed for ivar reflection).
@interface LynxTemplateRender : NSObject
- (nullable LynxUIOwner*)uiOwner;
@end

// LynxUIContext — exposes the event handler. The real header declares
// it on a `+Internal` category; the bridge only needs the accessor.
@interface LynxUIContext : NSObject
@property (nonatomic, readonly, nullable) LynxEventHandler* eventHandler;
@end

// LynxUIOwner — carries the UI context.
@interface LynxUIOwner : NSObject
@property (nonatomic, readonly, nullable) LynxUIContext* uiContext;
@end

// LynxEventHandler — owns the emitter the bridge hooks into.
@interface LynxEventHandler : NSObject
@property (nonatomic, readonly, nullable) LynxEventEmitter* eventEmitter;
@end

// LynxEvent — base class. The bridge reads `eventName` / `targetSign`,
// calls `generateEventBody`, and isKindOfClass-checks the touch
// subclass.
@interface LynxEvent : NSObject
@property (nonatomic, readonly, copy, nullable) NSString* eventName;
@property (nonatomic, readonly) NSInteger targetSign;
- (nullable NSMutableDictionary*)generateEventBody;
@end

// LynxEventEmitter — installs the reporter block that returns YES if the
// event was consumed and the native chain should stop.
@interface LynxEventEmitter : NSObject
- (void)setEventReporterBlock:(BOOL (^)(LynxEvent* event))block;
@end

// LynxTouchEvent — touch coordinate carrier. Multi-touch path goes
// through `touchMap` keyed by identifier; single-touch path reads
// `pagePoint` / `clientPoint` directly.
@interface LynxTouchEvent : LynxEvent
@property (nonatomic, readonly) BOOL isMultiTouch;
@property (nonatomic, readonly) CGPoint pagePoint;
@property (nonatomic, readonly) CGPoint clientPoint;
@property (nonatomic, readonly, nullable) NSDictionary* touchMap;
@end

NS_ASSUME_NONNULL_END

#endif  // __OBJC__

#endif  // WHISKER_BRIDGE_LYNX_OBJC_STUBS_H_
