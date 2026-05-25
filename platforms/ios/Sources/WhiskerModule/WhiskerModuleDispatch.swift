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
}

