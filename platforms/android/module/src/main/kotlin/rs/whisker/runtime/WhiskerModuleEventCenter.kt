// Android module event routing. The active Host installs a Kotlin sink rather
// than requiring modules to know about a JNI or renderer implementation.

package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap

/** Process-wide module registry and Host-injected event sink. */
public object WhiskerModuleEventCenter {

    /**
     * `qualifiedName → Module` lookup the JNI trampolines consult
     * to find the OnStart / OnStop closures for an incoming
     * `(module, event)` event.
     */
    private val modulesByName = ConcurrentHashMap<String, Module>()
    @Volatile
    private var eventSink: ((String, String, WhiskerValue) -> Unit)? = null

    /**
     * Register [module] after its qualified name has been assigned.
     * Idempotent — re-registering replaces the previous entry.
     */
    @JvmStatic
    public fun register(module: Module) {
        val qname = module.qualifiedName ?: return
        modulesByName[qname] = module
    }

    /** Install the event consumer owned by the active Whisker Host. */
    @JvmStatic
    public fun installEventSink(sink: ((String, String, WhiskerValue) -> Unit)?) {
        eventSink = sink
    }

    /** Forward a module event to the active Host, if one is installed. */
    internal fun dispatchSend(
        moduleName: String,
        eventName: String,
        payload: WhiskerValue,
    ) {
        eventSink?.invoke(moduleName, eventName, payload)
    }

    /**
     * Notify the registered module that its listener count changed 0 → 1.
     * Hosts call this when wiring Rust-side subscriptions.
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

}
