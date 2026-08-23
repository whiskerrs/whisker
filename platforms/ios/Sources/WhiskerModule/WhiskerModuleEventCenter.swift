// iOS module event routing. The active Host installs a Swift closure rather
// than exposing its renderer or FFI implementation to module authors.

import Foundation

/// Shared dispatcher + observer-hook router. All state is internal
/// to the `WhiskerModule` framework; `Module.sendEvent` and the
/// codegen-emitted registration call are the only public entry
/// points.
public enum WhiskerModuleEventCenter {

    // MARK: - Registry

    /// Locked map of `qualifiedName → Module` instance the shared
    /// observer-hook trampolines consult to find the OnStart /
    /// OnStop closure for an incoming `(module, event)` event.
    private static let lock = NSLock()
    private static var modulesByName: [String: Module] = [:]
    private static var eventSink: ((String, String, WhiskerValue) -> Void)?

    // MARK: - Module registration

    /// Register `module` with the event center. Idempotent — a
    /// second call with the same qualified name replaces the previous
    /// registration (last-write-wins; useful for hot-reload).
    ///
    /// Authors don't call this directly — the codegen-emitted
    /// `_whiskerRegisterModules_<target>()` calls it after
    /// assigning `module.qualifiedName`.
    public static func register(_ module: Module) {
        guard let qname = module.qualifiedName else {
            #if DEBUG
            print("WhiskerModule: register() called on module without qualifiedName — skipped.")
            #endif
            return
        }
        lock.lock()
        modulesByName[qname] = module
        lock.unlock()

    }

    /// Install the process-wide event sink owned by the active Whisker
    /// runtime. Keeping this as a Swift closure avoids a second, legacy C
    /// registration ABI beside WhiskerView's runtime ABI.
    public static func installEventSink(
        _ sink: ((String, String, WhiskerValue) -> Void)?
    ) {
        lock.lock()
        eventSink = sink
        lock.unlock()
    }

    // MARK: - sendEvent

    /// Encode `payload`, dispatch through the bridge, then release
    /// the heap allocations. Called from `Module.sendEvent`.
    internal static func dispatchSend(
        module: String,
        event: String,
        payload: WhiskerValue
    ) {
        lock.lock()
        let sink = eventSink
        lock.unlock()
        sink?(module, event, payload)
    }

    // MARK: - Observer hook routing

    /// Look up the Module + event-name pair and fire any matching
    /// `OnStartObserving` closures. Called by the shared C
    /// trampoline below.
    public static func fireStart(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStartObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }

    /// Counterpart to `fireStart`.
    public static func fireStop(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStopObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }
}
