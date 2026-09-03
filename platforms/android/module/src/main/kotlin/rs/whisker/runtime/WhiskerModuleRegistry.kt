package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap

/// Signature of the dispatch closure each module's KSP-generated
/// `<Class>_Dispatch` object exposes. Lifted to file scope —
/// Kotlin disallows nested `typealias` declarations.
public typealias WhiskerModuleDispatchFn = (String, Array<WhiskerValue>) -> WhiskerValue

/// Signature of a module's ASYNC dispatch closure. Given a method,
/// its args, and a [WhiskerPromise] to resolve. Returns true if an
/// `AsyncFunction` with that name exists (it was invoked and owns the
/// promise); false if not — the Host then falls back to sync dispatch.
public typealias WhiskerModuleAsyncDispatchFn =
    (String, Array<WhiskerValue>, WhiskerPromise) -> Boolean

/**
 * Whisker native-module dispatch registry — Kotlin side.
 *
 * A `(module-name → dispatch lambda)` table. KSP generates a
 * `<Module>_Dispatch` object whose static `dispatch(method, args)`
 * resolves the right Kotlin method via a `when (method)` switch and
 * returns a [WhiskerValue].
 *
 * `WhiskerView` routes calls from its retained Rust runtime through these
 * functions. Module implementations remain unaware of the transport.
 */
public object WhiskerModuleRegistry {
    private val dispatchers = ConcurrentHashMap<String, WhiskerModuleDispatchFn>()
    private val asyncDispatchers = ConcurrentHashMap<String, WhiskerModuleAsyncDispatchFn>()

    /**
     * Register a dispatch closure under [name]. Subsequent
     * [invokeDispatch] calls with matching name route through it.
     * Replaces any previously-registered dispatcher (last-write-wins).
     *
     * Called at app launch by the KSP-generated registration code
     * (which itself is invoked from the user app's `Application.
     * onCreate()` via `WhiskerModuleBehaviors.registerAll()`).
     */
    @JvmStatic
    public fun registerDispatch(name: String, dispatch: WhiskerModuleDispatchFn) {
        dispatchers[name] = dispatch
    }

    /** Register the ASYNC dispatch closure for [name]. Parallel to
     *  [registerDispatch]; consulted first by [invokeDispatchAsync]. */
    @JvmStatic
    public fun registerDispatchAsync(name: String, dispatch: WhiskerModuleAsyncDispatchFn) {
        asyncDispatchers[name] = dispatch
    }

    /**
     * Invoke the registered dispatcher for [moduleName] with [method] and
     * [args].
     *
     * Returns `WhiskerValue.Err(...)` rather than throwing on
     * unknown module — keeps the wire shape uniform across happy /
     * sad paths and avoids `ExceptionDescribe` round-trips.
     */
    @JvmStatic
    public fun invokeDispatch(
        moduleName: String,
        method: String,
        args: Array<WhiskerValue>,
    ): WhiskerValue {
        val fn = dispatchers[moduleName]
            ?: return WhiskerValue.Err("module not registered: $moduleName")
        return try {
            fn(method, args)
        } catch (t: Throwable) {
            WhiskerValue.Err("module $moduleName.$method threw: ${t.message ?: t.javaClass.simpleName}")
        }
    }

    /**
     * Async parallel of [invokeDispatch]. The Host supplies [onSettle], which
     * is wrapped in a one-shot [WhiskerPromise].
     *
     * Returns true if an async dispatcher owned the method (it will fire
     * the callback later); false if not, allowing the Host to fall back to
     * the sync path.
     */
    @JvmStatic
    public fun invokeDispatchAsync(
        moduleName: String,
        method: String,
        args: Array<WhiskerValue>,
        onSettle: (WhiskerValue) -> Unit,
    ): Boolean {
        val fn = asyncDispatchers[moduleName] ?: return false
        val promise = WhiskerPromise(onSettle)
        return try {
            fn(method, args, promise)
        } catch (t: Throwable) {
            // An async method existed but threw before/at invocation:
            // reject (resolving the awaiting future) and report as owned.
            promise.reject("module $moduleName.$method threw: ${t.message ?: t.javaClass.simpleName}")
            true
        }
    }

}
