// Android implementation of `WhiskerLocalStore` — backs the
// `whisker::native_module::invoke("WhiskerLocalStore", ...)` calls
// with `SharedPreferences` (private to the host app under the name
// "WhiskerLocalStore").
//
// Activated by `@WhiskerModule("WhiskerLocalStore")` — the
// `whisker-android-ksp` KSP processor finds the annotation at the
// user-app's compile time and emits a
// `WhiskerModuleRegistry.registerModuleClass(name, cls)` call into
// the auto-generated `WhiskerModuleBehaviors.kt`. The JNI bridge
// (`whisker_bridge_android.cc`) then looks up this class by name
// on every `invoke_module` from Rust.
//
// Method-shape contract (shared with iOS): one `Array<Any?>`
// argument + an `Any?` return; the JNI dispatch (see
// `whisker_bridge_android.cc::GetCachedMethod`) signature is
// `([Ljava/lang/Object;)Ljava/lang/Object;`. Returning `null`
// maps to `WhiskerValue::Null` Rust-side, which the proxy lifts
// into `Option::None` / `()` for the matching return type.
//
// Per-WhiskerView Context: the bridge instantiates the registered
// class via `cls.getDeclaredConstructor().newInstance()` — i.e.
// zero-arg ctor. We don't have access to a `Context` at
// construction time, so we resolve it lazily through
// `WhiskerApplication.appContext`. The generated WhiskerApplication
// stores a process-wide ApplicationContext reference at
// `onCreate()`, before any module dispatch happens, so this lookup
// is always safe by the time a method runs.

package rs.whisker.modules.localstore

import android.content.Context
import android.content.SharedPreferences
import rs.whisker.annotations.WhiskerModule
import rs.whisker.runtime.WhiskerApplication

@WhiskerModule("WhiskerLocalStore")
class WhiskerLocalStoreImpl {

    /** SharedPreferences-bucket name. App-private — not visible to other apps. */
    private val prefsName = "WhiskerLocalStore"

    /**
     * Resolve the app-private SharedPreferences instance. Lazy so
     * the no-arg ctor doesn't need an injected Context — the
     * Application has installed `appContext` before any dispatch
     * could reach this method.
     */
    private fun prefs(): SharedPreferences {
        val ctx = WhiskerApplication.appContext
            ?: throw IllegalStateException(
                "WhiskerLocalStore: WhiskerApplication.appContext not initialised — " +
                    "ensure your Application class extends WhiskerApplication " +
                    "and super.onCreate() runs before any module dispatch",
            )
        return ctx.getSharedPreferences(prefsName, Context.MODE_PRIVATE)
    }

    /** Save args[0] (String key) → args[1] (String value). Returns true on success. */
    fun save(args: Array<Any?>): Any {
        val key = args.getOrNull(0) as? String ?: return false
        val value = args.getOrNull(1) as? String ?: return false
        return prefs().edit().putString(key, value).commit()
    }

    /**
     * Load the value for args[0]. Returns the value String on hit,
     * null on miss — the JNI bridge converts null → WhiskerValue::Null,
     * which the proxy lifts into Option::None.
     */
    fun load(args: Array<Any?>): Any? {
        val key = args.getOrNull(0) as? String ?: return null
        return prefs().getString(key, null)
    }

    /** Drop args[0]'s entry. Returns null (→ `()` on the Rust side). */
    fun remove(args: Array<Any?>): Any? {
        val key = args.getOrNull(0) as? String ?: return null
        prefs().edit().remove(key).apply()
        return null
    }
}
