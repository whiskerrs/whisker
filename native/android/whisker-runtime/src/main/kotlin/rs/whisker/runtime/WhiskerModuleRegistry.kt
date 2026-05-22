package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap

/// Signature of the dispatch closure each module's KSP-generated
/// `<Class>_Dispatch` object exposes. Lifted to file scope —
/// Kotlin disallows nested `typealias` declarations.
public typealias WhiskerModuleDispatchFn = (String, Array<WhiskerValue>) -> WhiskerValue

/**
 * Whisker native-module dispatch registry — Kotlin side.
 *
 * Phase 7-Φ.F: replaces the per-module class registration + JNI
 * `GetMethodID` reflection pattern with a `(module-name → dispatch
 * lambda)` table. KSP generates a `<Module>_Dispatch` object whose
 * static `dispatch(method, args)` resolves the right Kotlin method
 * via a `when (method)` switch and returns a [WhiskerValue].
 *
 * The C bridge (`whisker_bridge_android.cc`) routes
 * `whisker_bridge_invoke_module` through [invokeDispatch] via a
 * single cached static-method JNI call, bypassing per-call
 * reflection.
 *
 * Pre-F the registry stored `(name → Class<*>)` and dispatched
 * methods by name via `Class.getMethod(name, Array<Any?>::class.java)`
 * on every call. That path is gone — the per-module dispatch
 * object subsumes both the class registration and the method
 * lookup.
 */
public object WhiskerModuleRegistry {
    private val dispatchers = ConcurrentHashMap<String, WhiskerModuleDispatchFn>()

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

    /**
     * Invoke the registered dispatcher for [moduleName] with the
     * given [method] and [args]. The C JNI bridge in
     * `whisker_bridge_android.cc` looks this static method up once
     * (cached `jmethodID`) and calls it on every
     * `whisker_bridge_invoke_module` from Rust.
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
}
