// iOS module event routing. The active Host installs a Swift closure rather
// than exposing its renderer or FFI implementation to module authors.

import Foundation

/// Shared dispatcher + observer-hook router. All state is internal
/// to the `WhiskerModule` framework; `Module.sendEvent` and the
/// codegen-emitted registration call are the only public entry
/// points.
public enum WhiskerModuleEventCenter {

    private struct EventKey: Hashable {
        let module: String
        let event: String
    }

    private final class SurfaceSink {
        var dispatch: (String, String, WhiskerValue) -> Void
        var observed: Set<EventKey> = []

        init(dispatch: @escaping (String, String, WhiskerValue) -> Void) {
            self.dispatch = dispatch
        }
    }

    // MARK: - Registry

    /// Locked map of `qualifiedName → Module` instance the shared
    /// observer-hook trampolines consult to find the OnStart /
    /// OnStop closure for an incoming `(module, event)` event.
    private static let lock = NSLock()
    private static var modulesByName: [String: Module] = [:]
    private static var eventSinks: [ObjectIdentifier: SurfaceSink] = [:]

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

    /// Install or remove the event sink owned by one Host surface.
    /// Keeping these as Swift closures avoids a second, legacy C registration
    /// ABI beside WhiskerView's runtime ABI.
    public static func installEventSink(
        owner: AnyObject,
        _ sink: ((String, String, WhiskerValue) -> Void)?
    ) {
        lock.lock()
        let key = ObjectIdentifier(owner)
        var stopped: [EventKey] = []
        if let sink {
            if let existing = eventSinks[key] {
                existing.dispatch = sink
            } else {
                eventSinks[key] = SurfaceSink(dispatch: sink)
            }
        } else {
            let removed = eventSinks.removeValue(forKey: key)
            stopped = removed?.observed.filter { event in
                !eventSinks.values.contains { $0.observed.contains(event) }
            } ?? []
        }
        lock.unlock()
        for event in stopped {
            fireStop(module: event.module, event: event.event)
        }
    }

    /// Update one surface's first/last Rust-side subscription.
    public static func setObserving(
        owner: AnyObject,
        module: String,
        event: String,
        observing: Bool
    ) {
        let key = EventKey(module: module, event: event)
        lock.lock()
        guard let surface = eventSinks[ObjectIdentifier(owner)] else {
            lock.unlock()
            return
        }
        let changed: Bool
        let transition: Bool
        if observing {
            changed = surface.observed.insert(key).inserted
            transition = changed && eventSinks.values.filter { $0.observed.contains(key) }.count == 1
        } else {
            changed = surface.observed.remove(key) != nil
            transition = changed && !eventSinks.values.contains { $0.observed.contains(key) }
        }
        lock.unlock()
        guard transition else { return }
        if observing {
            fireStart(module: module, event: event)
        } else {
            fireStop(module: module, event: event)
        }
    }

    // MARK: - sendEvent

    /// Encode `payload`, dispatch through the bridge, then release
    /// the heap allocations. Called from `Module.sendEvent`.
    internal static func dispatchSend(
        module: String,
        event: String,
        payload: WhiskerValue
    ) {
        let key = EventKey(module: module, event: event)
        lock.lock()
        let sinks = eventSinks.values
            .filter { $0.observed.contains(key) }
            .map(\.dispatch)
        lock.unlock()
        for sink in sinks {
            sink(module, event, payload)
        }
    }

    // MARK: - Observer hook routing

    /// Look up the Module + event-name pair and fire any matching
    /// `OnStartObserving` closures. Called by the shared C
    /// trampoline below.
    private static func fireStart(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStartObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }

    /// Counterpart to `fireStart`.
    private static func fireStop(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStopObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }
}
