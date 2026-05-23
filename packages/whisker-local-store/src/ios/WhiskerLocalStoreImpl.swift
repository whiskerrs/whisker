// iOS implementation of `WhiskerLocalStore` — backs the
// `whisker::native_module::invoke("WhiskerLocalStore", ...)` calls
// with `UserDefaults.standard`.
//
// Phase 7-Φ.F: the contract is now WhiskerValue-only. Each method
// takes `[WhiskerValue]` and returns `WhiskerValue`; the
// `@WhiskerModule` macro emits the `@_cdecl` dispatch shim that
// decodes args from the C ABI, switches on method name, and
// re-encodes the return for the bridge. No more Obj-C selectors,
// no `@objc(...)`, no `NSArray` / `NSObject` marshalling.
//
// The Rust-side type-safe wrapper (`WhiskerLocalStore::save(key,
// value)`) builds the `Vec<WhiskerValue>` from typed args; here
// we pattern-match on the `WhiskerValue` cases to extract the
// expected types and produce the matching `WhiskerValue` return.
// Anything that doesn't match — wrong arity, wrong variant —
// returns `.error("...")` so the Rust wrapper can lift it into
// `WhiskerModuleError`.

import Foundation
import WhiskerElements
import WhiskerRuntime

@WhiskerModule("WhiskerLocalStore")
public class WhiskerLocalStoreImpl {
    /// Save args[0] (String key) → args[1] (String value) in
    /// UserDefaults. Returns `Bool(true)` on success.
    func save(_ args: [WhiskerValue]) -> WhiskerValue {
        guard args.count >= 2,
              case .string(let key)   = args[0],
              case .string(let value) = args[1]
        else {
            return .error("WhiskerLocalStore.save expects (String, String)")
        }
        UserDefaults.standard.set(value, forKey: key)
        return .bool(true)
    }

    /// Load the value for args[0] (String key). Returns the value
    /// as `.string(_)` on hit, `.null` on miss. The Rust wrapper
    /// lifts `.null` into `Option::None`.
    func load(_ args: [WhiskerValue]) -> WhiskerValue {
        guard args.count >= 1,
              case .string(let key) = args[0]
        else {
            return .error("WhiskerLocalStore.load expects (String,)")
        }
        if let value = UserDefaults.standard.string(forKey: key) {
            return .string(value)
        }
        return .null
    }

    /// Remove args[0]'s entry. Returns `.null` (→ `()` Rust-side).
    func remove(_ args: [WhiskerValue]) -> WhiskerValue {
        guard args.count >= 1,
              case .string(let key) = args[0]
        else {
            return .error("WhiskerLocalStore.remove expects (String,)")
        }
        UserDefaults.standard.removeObject(forKey: key)
        return .null
    }
}
