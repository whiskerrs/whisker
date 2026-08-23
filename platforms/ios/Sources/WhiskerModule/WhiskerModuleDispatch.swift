// Module-level (view-less) `Function` dispatch (iOS). The active Host looks
// up the registered Module and invokes these platform-neutral Swift methods.

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
        // Module-level functions get `nil` for the view argument.
        return fn.handler(nil, args)
    }

    /// Dispatch a module-level `AsyncFunction` by name, handing it a
    /// `WhiskerPromise` to resolve. Returns true if such an async function
    /// exists (it was invoked and owns the promise); false if not — the
    /// Host can then fall back to the sync path. Public for unit-testing.
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

}
