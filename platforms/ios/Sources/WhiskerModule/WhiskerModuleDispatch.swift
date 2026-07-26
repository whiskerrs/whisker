// Phase L-3 — module-level (view-less) `Function` dispatch (iOS).
//
// For view-less DSL modules (no `View(...)` block), the SwiftPM
// codegen plugin registers a `@_cdecl` shim with the C bridge via
// `whisker_bridge_register_module_dispatch(name, shim)`. The shim
// calls `dispatchModuleFunctionRaw(...)`, which decodes the C args
// into `[WhiskerValue]`, looks the function up in `definitionLazy`
// by name, runs the closure, and re-encodes the result.
//
// Kept in its own file (separate from `WhiskerModuleRegistrar.swift`)
// because it pulls in `WhiskerValue` / `WhiskerValueRaw` — the
// C-ABI surface that can't be host-compiled. The L-2b view-install
// path in the registrar stays free of that dependency so its
// runtime smoke test keeps building on plain host macOS.

import Foundation

extension Module {
    /// Dispatch a module-level `Function` by name. Public so it's
    /// unit-testable against `[WhiskerValue]` without the C ABI.
    public func dispatchModuleFunction(
        _ method: String,
        _ args: [WhiskerValue]
    ) -> WhiskerValue {
        guard let fn = self.definitionLazy.functions.first(where: { $0.name == method }) else {
            return .error("unknown method `\(method)`")
        }
        // Case ②: raw `[WhiskerValue]` straight through. Module-level
        // functions get `nil` for the view argument.
        return fn.handler(nil, args)
    }

    /// C-ABI bridge the codegen-emitted `@_cdecl` shim calls. Lives
    /// in `WhiskerModule` (same module as `WhiskerValue`) so it
    /// can reach the `internal` `decodeArray` / `toRaw` helpers —
    /// keeping those off the public surface while still letting the
    /// generated shim (in the module author's target) dispatch with
    /// a single public call.
    public func dispatchModuleFunctionRaw(
        _ methodName: UnsafePointer<CChar>?,
        _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
        _ argCount: Int
    ) -> WhiskerValueRaw {
        let method = methodName == nil ? "" : String(cString: methodName!)
        let args = WhiskerValue.decodeArray(argsPtr, count: argCount)
        return dispatchModuleFunction(method, args).toRaw()
    }

    /// Dispatch a module-level `AsyncFunction` by name, handing it a
    /// `WhiskerPromise` to resolve. Returns true if such an async function
    /// exists (it was invoked and owns the promise); false if not — the
    /// bridge then falls back to the sync path. Public for unit-testing
    /// against `[WhiskerValue]` without the C ABI.
    public func dispatchModuleFunctionAsync(
        _ method: String,
        _ args: [WhiskerValue],
        _ promise: WhiskerPromise
    ) -> Bool {
        guard
            let fn = self.definitionLazy.asyncFunctions.first(where: { $0.name == method })
        else {
            return false
        }
        fn.handler(nil, args, promise)
        return true
    }

    /// C-ABI async bridge the codegen-emitted `@_cdecl` async shim calls
    /// (matches `WhiskerModuleAsyncDispatchFn`). Returns true if an async
    /// function with that name owned the call. On unknown method it returns
    /// false WITHOUT touching the callback, so the bridge sync-forwards.
    public func dispatchModuleFunctionRawAsync(
        _ methodName: UnsafePointer<CChar>?,
        _ argsPtr: UnsafePointer<WhiskerValueRaw>?,
        _ argCount: Int,
        _ callback: WhiskerModuleCallback?,
        _ userData: UnsafeMutableRawPointer?
    ) -> Bool {
        guard let callback = callback else { return false }
        let method = methodName == nil ? "" : String(cString: methodName!)
        // Look up before constructing the promise so a non-async method
        // leaves the callback untouched for the bridge's sync fallback.
        guard self.definitionLazy.asyncFunctions.contains(where: { $0.name == method }) else {
            return false
        }
        let args = WhiskerValue.decodeArray(argsPtr, count: argCount)
        let promise = WhiskerPromise(callback: callback, userData: userData)
        return dispatchModuleFunctionAsync(method, args, promise)
    }
}

