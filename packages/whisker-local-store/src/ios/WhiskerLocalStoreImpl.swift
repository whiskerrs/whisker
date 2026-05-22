// iOS implementation of `WhiskerLocalStore` — backs the
// `whisker::native_module::invoke("WhiskerLocalStore", ...)` calls
// with `UserDefaults.standard`.
//
// Activated by the `@WhiskerModule("WhiskerLocalStore")` annotation
// — the `WhiskerElementsCodegen` SwiftPM build-tool plugin scans
// for it at build time and emits a
// `WhiskerModuleRegistry.registerModuleClass(_:forName:)` call into
// the auto-generated `WhiskerModuleBehaviors.swift`. The C bridge's
// `whisker_bridge_invoke_module` then looks up this class by name
// when the Rust proxy dispatches.
//
// Method shape contract (shared with Android):
//   Each method takes a single `NSArray*` arg and returns `NSObject?`.
//   The Obj-C runtime selector is `<methodName>:` — the C bridge
//   appends the colon when building its NSInvocation.
//
// Args + returns marshal via `WhiskerValueToFoundation` /
// `FoundationToWhiskerValue` (see `whisker_bridge_ios.mm`):
//
//   Rust          C ABI            Foundation           Swift here
//   --------------------------------------------------------------
//   String  →  WHISKER_VALUE_STRING → NSString    →  args[i] as String
//   bool    ←  WHISKER_VALUE_BOOL   ← NSNumber    ←  NSNumber(value: true)
//   None    ←  WHISKER_VALUE_NULL   ← NSNull      ←  NSNull() (or return nil)
//
// The proxy on the Rust side handles dispatch failures (unknown
// module / wrong return type) by returning
// `WhiskerValue::Error("...")`, so this Swift code only needs to
// handle the happy path + arg-shape mismatches.

import Foundation
import WhiskerElements

@WhiskerModule("WhiskerLocalStore")
@objc(WhiskerLocalStoreImpl)
public class WhiskerLocalStoreImpl: NSObject {
    /// Save args[0] (String key) → args[1] (String value) in
    /// UserDefaults. Returns NSNumber(true) on success.
    @objc public func save(_ args: NSArray) -> NSObject {
        guard
            args.count >= 2,
            let key = args[0] as? String,
            let value = args[1] as? String
        else {
            return NSNumber(value: false)
        }
        UserDefaults.standard.set(value, forKey: key)
        return NSNumber(value: true)
    }

    /// Load the value for args[0] (String key). Returns the value
    /// as NSString on success, NSNull on no-such-key — the bridge's
    /// FoundationToWhiskerValue maps NSNull → WhiskerValue::Null,
    /// which the `#[whisker::native_module]` proxy lifts into
    /// `Option::None`.
    @objc public func load(_ args: NSArray) -> NSObject {
        guard
            args.count >= 1,
            let key = args[0] as? String
        else {
            return NSNull()
        }
        if let value = UserDefaults.standard.string(forKey: key) {
            return value as NSString
        }
        return NSNull()
    }

    /// Remove args[0]'s entry from UserDefaults. Returns NSNull
    /// (bridge → WhiskerValue::Null → `()` on the Rust side).
    @objc public func remove(_ args: NSArray) -> NSObject {
        guard
            args.count >= 1,
            let key = args[0] as? String
        else {
            return NSNull()
        }
        UserDefaults.standard.removeObject(forKey: key)
        return NSNull()
    }
}
