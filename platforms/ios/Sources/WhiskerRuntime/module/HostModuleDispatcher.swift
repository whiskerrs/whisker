import WhiskerModule

/// Routes runtime module calls through the Host-local module registry.
final class HostModuleDispatcher {
    func invoke(
        module name: String,
        method: String,
        rawArgs: UnsafePointer<WhiskerValueRaw>?,
        argumentCount: Int,
        isAsync: Bool,
        result: @escaping WhiskerModuleResult,
        resultData: UnsafeMutableRawPointer?
    ) -> Bool {
        guard let module = WhiskerModuleRegistry.module(named: name) else {
            deliver(.error("module not registered: \(name)"), result, resultData)
            return true
        }
        let args = WhiskerValue.decodeArray(rawArgs, count: argumentCount)
        let settle: (WhiskerValue) -> Void = { [weak self] value in
            self?.deliver(value, result, resultData)
        }
        if isAsync {
            let promise = WhiskerPromise(onSettle: settle)
            if module.dispatchModuleFunctionAsync(method, args, promise) { return true }
        }
        settle(module.dispatchModuleFunction(method, args))
        return true
    }

    func observe(module: String, event: String, observing: Bool) {
        if observing {
            WhiskerModuleEventCenter.fireStart(module: module, event: event)
        } else {
            WhiskerModuleEventCenter.fireStop(module: module, event: event)
        }
    }

    private func deliver(
        _ value: WhiskerValue,
        _ result: WhiskerModuleResult,
        _ resultData: UnsafeMutableRawPointer?
    ) {
        var raw = value.toRaw()
        result(resultData, &raw)
        WhiskerValue.releaseRaw(&raw)
    }
}
