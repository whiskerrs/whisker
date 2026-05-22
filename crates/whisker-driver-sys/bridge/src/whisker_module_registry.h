// `WhiskerModuleRegistry` — Whisker-owned native-module name→class
// map on iOS.
//
// Lynx has its own `LynxModule` registration path for the JS engine;
// Whisker keeps a parallel map keyed by Whisker's module names so
// the C bridge can find a module class by string without round-
// tripping through Lynx's JSValue dispatch (which would also
// require a JS engine to be alive — Whisker has none).
//
// Modules are registered at app launch by the auto-generated
// `WhiskerModuleRegistration.swift` (the SwiftPM `BuildToolPlugin`
// adds `@WhiskerModule` discovery in Phase 7-Φ.E.6). Until then,
// modules can register manually from Swift / Obj-C via
// `[WhiskerModuleRegistry registerModuleClass:forName:]`.
//
// Instances are lazy-singleton — one shared instance per registered
// class, created on first lookup. Module authors who need
// per-WhiskerView instances should drop down to manual lookup +
// allocation from the consuming code instead of using the bridge's
// shared-instance path.

#ifndef WHISKER_MODULE_REGISTRY_H_
#define WHISKER_MODULE_REGISTRY_H_

#ifdef __OBJC__

#import <Foundation/Foundation.h>

NS_ASSUME_NONNULL_BEGIN

@interface WhiskerModuleRegistry : NSObject

/// Register a module class under `name`. Subsequent
/// `moduleInstanceForName:`/`moduleClassForName:` calls resolve to
/// `cls`. Replaces any previously-registered class for the same
/// name — last-write-wins, mirroring Lynx's own UI-class registry
/// semantics.
+ (void)registerModuleClass:(Class)cls forName:(NSString*)name;

/// Resolve a module name to its registered class, or nil if no
/// registration matches.
+ (nullable Class)moduleClassForName:(NSString*)name;

/// Resolve a module name to its shared instance (creating one
/// lazily via `[[cls alloc] init]` on first lookup). Returns nil
/// if no class is registered or the class has no zero-arg init.
+ (nullable id)moduleInstanceForName:(NSString*)name;

@end

NS_ASSUME_NONNULL_END

#endif  // __OBJC__

#endif  // WHISKER_MODULE_REGISTRY_H_
