// Android module event routing. The active Host installs a Kotlin sink rather
// than requiring modules to know about a JNI or renderer implementation.

package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap
import java.util.IdentityHashMap

/** Process-wide module registry and surface-scoped Host event subscriptions. */
public object WhiskerModuleEventCenter {

    private data class EventKey(val module: String, val event: String)

    private class SurfaceSink(
        var dispatch: (String, String, WhiskerValue) -> Unit,
        val observed: MutableSet<EventKey> = mutableSetOf(),
    )

    /**
     * `qualifiedName → Module` lookup the JNI trampolines consult
     * to find the OnStart / OnStop closures for an incoming
     * `(module, event)` event.
     */
    private val modulesByName = ConcurrentHashMap<String, Module>()
    private val eventSinkLock = Any()
    private val eventSinks = IdentityHashMap<Any, SurfaceSink>()

    /**
     * Register [module] after its qualified name has been assigned.
     * Idempotent — re-registering replaces the previous entry.
     */
    @JvmStatic
    public fun register(module: Module) {
        val qname = module.qualifiedName ?: return
        modulesByName[qname] = module
    }

    /** Install or remove the event consumer owned by one Host surface. */
    @JvmStatic
    public fun installEventSink(
        owner: Any,
        sink: ((String, String, WhiskerValue) -> Unit)?,
    ) {
        val stopped = synchronized(eventSinkLock) {
            if (sink == null) {
                val removed = eventSinks.remove(owner) ?: return@synchronized emptyList()
                removed.observed.filterTo(mutableListOf()) { key ->
                    eventSinks.values.none { key in it.observed }
                }
            } else {
                val existing = eventSinks[owner]
                if (existing == null) {
                    eventSinks[owner] = SurfaceSink(sink)
                } else {
                    existing.dispatch = sink
                }
                emptyList()
            }
        }
        stopped.forEach { key -> fireStop(key.module, key.event) }
    }

    /** Update one surface's first/last Rust-side subscription. */
    @JvmStatic
    public fun setObserving(
        owner: Any,
        moduleName: String,
        eventName: String,
        observing: Boolean,
    ) {
        val key = EventKey(moduleName, eventName)
        val transition = synchronized(eventSinkLock) {
            val surface = eventSinks[owner] ?: return
            if (observing) {
                if (!surface.observed.add(key)) return
                eventSinks.values.count { key in it.observed } == 1
            } else {
                if (!surface.observed.remove(key)) return
                eventSinks.values.none { key in it.observed }
            }
        }
        if (transition) {
            if (observing) fireStart(moduleName, eventName) else fireStop(moduleName, eventName)
        }
    }

    /** Forward an event only to surfaces currently observing its channel. */
    internal fun dispatchSend(
        moduleName: String,
        eventName: String,
        payload: WhiskerValue,
    ) {
        val key = EventKey(moduleName, eventName)
        val sinks = synchronized(eventSinkLock) {
            eventSinks.values.filter { key in it.observed }.map { it.dispatch }
        }
        if (sinks.isEmpty()) return
        val snapshot = payload.snapshotForDispatch()
        sinks.forEach { sink -> sink(moduleName, eventName, snapshot) }
    }

    /**
     * Notify the registered module that its listener count changed 0 → 1.
     * Hosts call this when wiring Rust-side subscriptions.
     */
    @JvmStatic
    internal fun fireStart(moduleName: String, eventName: String) {
        modulesByName[moduleName]?.fireOnStartObserving(eventName)
    }

    /** Counterpart to [fireStart] — fires on 1 → 0 transitions. */
    @JvmStatic
    internal fun fireStop(moduleName: String, eventName: String) {
        modulesByName[moduleName]?.fireOnStopObserving(eventName)
    }

}

/** Own mutable byte/container storage before crossing an asynchronous Host turn. */
private fun WhiskerValue.snapshotForDispatch(): WhiskerValue = when (this) {
    WhiskerValue.Null -> this
    is WhiskerValue.Bool -> this
    is WhiskerValue.Int -> this
    is WhiskerValue.Float -> this
    is WhiskerValue.Str -> this
    is WhiskerValue.Bytes -> WhiskerValue.Bytes(value.copyOf())
    is WhiskerValue.Array -> WhiskerValue.Array(value.map { it.snapshotForDispatch() })
    is WhiskerValue.Map -> WhiskerValue.Map(
        value.entries.associate { (key, item) -> key to item.snapshotForDispatch() },
    )
    is WhiskerValue.Err -> this
}
