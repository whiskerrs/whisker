// Phase L-2c — iOS event subscription wiring.
//
// Sits between the Rust subscription API
// (`PlatformModule::on_event`) and the `Module` author's
// `Events("name")` + `OnStartObserving` / `OnStopObserving` DSL
// surface.
//
// ## Roles
//
// 1. **sendEvent dispatch.** `Module.sendEvent(name, payload)` calls
//    `dispatchSend(...)`, which encodes the payload as a heap-owned
//    `WhiskerValueRaw`, hands it to the bridge, and releases the
//    allocations once the bridge returns. The bridge synchronously
//    fans the payload out to every Rust subscriber registered against
//    `(module.qualifiedName, event)`.
//
// 2. **Observer-hook routing.** The bridge's
//    `whisker_bridge_module_register_observer_hooks` takes a pair of
//    `void(*)(const char*, const char*)` callbacks. C function
//    pointers can't capture state, so we install ONE shared C
//    trampoline pair (`sharedStartHook` / `sharedStopHook`) at
//    process startup and route the (module, event) arguments through
//    this center's `[qualifiedName: Module]` registry to reach the
//    right `OnStartObserving` / `OnStopObserving` closure.
//
//    The codegen-emitted registration block calls
//    `register(_ module:)` once per `Module` subclass after assigning
//    its `qualifiedName`. The first registration also wires the
//    shared trampolines into the bridge (lazy install).

import Foundation
@_exported import WhiskerCBridge

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

        // Wire the shared observer-hook trampolines into the bridge
        // for this module. The bridge keeps one (started, stopped)
        // pair per module name; passing the same shared pair for
        // every module is what lets the trampoline route by module
        // name back through `modulesByName`.
        qname.withCString { moduleC in
            whisker_bridge_module_register_observer_hooks(
                moduleC,
                sharedStartHook,
                sharedStopHook
            )
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
        var raw = payload.toRaw()
        module.withCString { moduleC in
            event.withCString { eventC in
                whisker_bridge_module_send_event(moduleC, eventC, &raw)
            }
        }
        // Bridge fans out synchronously inside `module_send_event` and
        // doesn't retain `raw`; safe to release now.
        whisker_bridge_value_release(&raw)
    }

    // MARK: - Observer hook routing

    /// Look up the Module + event-name pair and fire any matching
    /// `OnStartObserving` closures. Called by the shared C
    /// trampoline below.
    fileprivate static func fireStart(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStartObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }

    /// Counterpart to `fireStart`.
    fileprivate static func fireStop(module: String, event: String) {
        lock.lock()
        let m = modulesByName[module]
        lock.unlock()
        guard let m else { return }
        for hook in m.definitionLazy.onStopObservingHooks where hook.eventName == event {
            hook.handler()
        }
    }
}

// MARK: - Shared C trampolines

/// Bridge's `started` callback. Decodes the (module, event) pair and
/// fires every `OnStartObserving("event")` block declared on the
/// matching module. Process-global because the bridge stores the
/// function pointer directly; one shared trampoline is enough since
/// the `module` argument is the routing key.
private let sharedStartHook: WhiskerModuleObserverHook = { moduleC, eventC in
    guard let moduleC, let eventC else { return }
    let module = String(cString: moduleC)
    let event = String(cString: eventC)
    WhiskerModuleEventCenter.fireStart(module: module, event: event)
}

private let sharedStopHook: WhiskerModuleObserverHook = { moduleC, eventC in
    guard let moduleC, let eventC else { return }
    let module = String(cString: moduleC)
    let event = String(cString: eventC)
    WhiskerModuleEventCenter.fireStop(module: module, event: event)
}
