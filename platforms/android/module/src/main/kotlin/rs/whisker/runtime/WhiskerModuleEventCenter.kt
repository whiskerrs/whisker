// Phase L-2c — Android event-subscription wiring.
//
// Sits between the Rust subscription API (`PlatformModule::on_event`)
// and the `Module` author's `Events("name")` +
// `OnStartObserving` / `OnStopObserving` DSL.
//
// ## Roles
//
// 1. **sendEvent dispatch.** `Module.sendEvent(name, payload)` calls
//    [dispatchSend], which routes through JNI into
//    `whisker_bridge_module_send_event`. The bridge synchronously
//    fans the payload out to every Rust subscriber registered against
//    `(module.qualifiedName, event)`.
//
// 2. **Observer-hook routing.** When a `Module` is registered, the
//    KSP-generated code calls [register]. The center stores a
//    `qualifiedName → Module` mapping and (via a JNI native method)
//    asks the C++ bridge to point its per-module observer hooks at
//    the shared trampolines below. The trampolines route incoming
//    `(module, event)` events back to the matching `Module`'s
//    `fireOnStartObserving` / `fireOnStopObserving`.

package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap

/**
 * Process-wide dispatcher + observer-hook router. All public
 * entry points are `@JvmStatic` so the C++ JNI bridge can
 * invoke them via a single cached `jmethodID`.
 */
public object WhiskerModuleEventCenter {

    /**
     * `qualifiedName → Module` lookup the JNI trampolines consult
     * to find the OnStart / OnStop closures for an incoming
     * `(module, event)` event.
     */
    private val modulesByName = ConcurrentHashMap<String, Module>()

    /**
     * Register [module] with the event center. The KSP-generated
     * `_whiskerRegisterModules()` calls this after assigning
     * `module.qualifiedName`. Idempotent — re-registering replaces
     * the previous entry (useful for hot-reload).
     */
    @JvmStatic
    public fun register(module: Module) {
        val qname = module.qualifiedName
        // DIAG (temporary): confirm register() runs + the qname, since the
        // C++ observer-hook table must be populated here BEFORE any Rust
        // `on_event` arrives or OnStartObserving never fires.
        android.util.Log.e("WhiskerPB", "EventCenter.register(qname=$qname)")
        if (qname == null) {
            android.util.Log.e("WhiskerPB", "EventCenter.register: qualifiedName is null — SKIPPED")
            return
        }
        modulesByName[qname] = module
        // Wire the C++ bridge's per-module observer hooks for this
        // module. The native side stores a `(module → started,
        // stopped)` table and fires the shared trampolines below.
        try {
            nativeRegisterObserverHooks(qname)
            android.util.Log.e("WhiskerPB", "EventCenter.register: nativeRegisterObserverHooks($qname) returned OK")
        } catch (t: Throwable) {
            android.util.Log.e("WhiskerPB", "EventCenter.register: nativeRegisterObserverHooks($qname) THREW: $t")
        }
    }

    /**
     * Encode [payload] and dispatch through the bridge. Called by
     * [Module.sendEvent].
     */
    internal fun dispatchSend(
        moduleName: String,
        eventName: String,
        payload: WhiskerValue,
    ) {
        nativeSendEvent(moduleName, eventName, payload)
    }

    /**
     * JNI trampoline target — invoked by the C++ bridge when a
     * `(module, event)` listener count goes 0 → 1. Looks up the
     * `Module` and fires every matching `OnStartObserving` closure.
     *
     * `@JvmStatic` + a flat name so the bridge can cache one
     * `jmethodID` via `GetStaticMethodID`.
     */
    @JvmStatic
    public fun fireStart(moduleName: String, eventName: String) {
        modulesByName[moduleName]?.fireOnStartObserving(eventName)
    }

    /** Counterpart to [fireStart] — fires on 1 → 0 transitions. */
    @JvmStatic
    public fun fireStop(moduleName: String, eventName: String) {
        modulesByName[moduleName]?.fireOnStopObserving(eventName)
    }

    // ----- Native methods --------------------------------------------------
    //
    // The C++ implementations live in
    // `crates/whisker-driver-sys/bridge/src/whisker_bridge_android.cc`
    // and follow the standard `Java_<fq-class>_<method>` JNI naming
    // convention (no `RegisterNatives` call needed).

    /**
     * Tell the bridge to point its per-module observer hooks for
     * [qualifiedName] at the shared trampolines that ultimately
     * route back into [fireStart] / [fireStop].
     */
    @JvmStatic
    private external fun nativeRegisterObserverHooks(qualifiedName: String)

    /**
     * Synchronously fan [payload] out to every Rust subscriber of
     * `(qualifiedName, eventName)` via the C bridge.
     */
    @JvmStatic
    private external fun nativeSendEvent(
        qualifiedName: String,
        eventName: String,
        payload: WhiskerValue,
    )
}
