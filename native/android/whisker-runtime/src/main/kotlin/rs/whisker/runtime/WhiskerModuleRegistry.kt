package rs.whisker.runtime

import java.util.concurrent.ConcurrentHashMap

/**
 * Android counterpart of iOS's `WhiskerModuleRegistry` Obj-C class.
 *
 * Name → class map keyed by the module's registration string, with
 * lazy-singleton instances. The native bridge (`whisker_bridge_android.cc`)
 * reaches in via JNI on every `invoke_module` call to resolve the
 * class and dispatch a method.
 *
 * Phase 7-Φ.E.3: the dispatch happens through JNI's
 * `CallObjectMethodA` with a cached `jmethodID`, against methods
 * declared with a single `Array<Any?>` argument and an `Any?`
 * return (the same wire shape iOS's `NSInvocation` dispatch uses).
 * Type-safety is recovered at the consuming end by the
 * `#[whisker::native_module]` proc macro (Phase 7-Φ.E.5).
 *
 * Authoring code calls [registerModuleClass] at app launch. Phase
 * 7-Φ.E.6 adds auto-generation of those calls from `@WhiskerModule`
 * Kotlin annotations via KSP; until then, modules register
 * manually from `WhiskerApplication.onCreate()` or similar.
 *
 * The instance map keeps a single shared instance per module name
 * — most native module classes are stateless service shims backed
 * by platform APIs (SharedPreferences, network, etc.). Modules
 * needing per-WhiskerView instances should bypass the bridge and
 * use the underlying platform API directly from their Kotlin
 * surface.
 */
public object WhiskerModuleRegistry {
    private val classes = ConcurrentHashMap<String, Class<*>>()
    private val instances = ConcurrentHashMap<String, Any>()

    /**
     * Register a module class under [name]. Subsequent
     * [classForName] / [instanceForName] calls resolve to [cls].
     * Replaces any previously-registered class for the same name —
     * last-write-wins, mirroring iOS's `WhiskerModuleRegistry`.
     */
    @JvmStatic
    public fun registerModuleClass(name: String, cls: Class<*>) {
        classes[name] = cls
        // Drop any cached instance — a fresh class replaces the
        // previous lazy-singleton.
        instances.remove(name)
    }

    /**
     * Resolve [name] to its registered class, or null if no
     * registration matches.
     */
    @JvmStatic
    public fun classForName(name: String): Class<*>? = classes[name]

    /**
     * Resolve [name] to its shared instance (creating one lazily
     * via `cls.getDeclaredConstructor().newInstance()` on first
     * lookup). Returns null if no class is registered or the class
     * has no public no-arg constructor.
     *
     * Called from the native bridge per `invoke_module` call; the
     * `ConcurrentHashMap` keeps the lookup lock-free in the common
     * already-cached case.
     */
    @JvmStatic
    public fun instanceForName(name: String): Any? {
        instances[name]?.let { return it }
        val cls = classes[name] ?: return null
        return try {
            val instance = cls.getDeclaredConstructor().newInstance()
            // Use putIfAbsent for the race where two threads both
            // pass the cache miss simultaneously — last write loses,
            // we return whichever instance won the race.
            instances.putIfAbsent(name, instance) ?: instance
        } catch (e: Throwable) {
            null
        }
    }
}
