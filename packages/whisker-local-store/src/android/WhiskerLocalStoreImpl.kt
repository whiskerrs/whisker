// Android implementation of `WhiskerLocalStore` — backs the
// `whisker::native_module::invoke("WhiskerLocalStore", ...)` calls
// with `SharedPreferences` (private to the host app under the name
// "WhiskerLocalStore").
//
// Phase 7-Φ.F: the contract is now WhiskerValue-only. Each method
// takes `Array<WhiskerValue>` and returns `WhiskerValue`. The KSP
// processor (`packages/whisker-android-ksp`) emits a
// `WhiskerLocalStoreImpl_Dispatch` object whose
// `dispatch(method, args)` `when`-switches on the method name and
// calls the matching instance method; the C JNI bridge resolves
// `WhiskerModuleRegistry.invokeDispatch` once per process and
// every `invoke_module` from Rust routes through the registry.
//
// Per-WhiskerView Context: the KSP-generated dispatch object
// constructs a single instance at `registerAll()` time
// (`<Class>()` invocation), so we still need a zero-arg ctor.
// The `Context` is resolved lazily via `WhiskerApplication.appContext`
// — the app's `Application.onCreate()` installs it before any
// module dispatch could reach this method.

package rs.whisker.modules.localstore

import android.content.Context
import android.content.SharedPreferences
import rs.whisker.annotations.WhiskerModule
import rs.whisker.runtime.WhiskerApplication
import rs.whisker.runtime.WhiskerValue

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

    /** Save args[0] (String key) → args[1] (String value). Returns Bool(true) on success. */
    fun save(args: Array<WhiskerValue>): WhiskerValue {
        if (args.size < 2) {
            return WhiskerValue.Err("WhiskerLocalStore.save expects (String, String)")
        }
        val key = args[0] as? WhiskerValue.Str
            ?: return WhiskerValue.Err("WhiskerLocalStore.save args[0] must be Str")
        val value = args[1] as? WhiskerValue.Str
            ?: return WhiskerValue.Err("WhiskerLocalStore.save args[1] must be Str")
        val ok = prefs().edit().putString(key.value, value.value).commit()
        return WhiskerValue.Bool(ok)
    }

    /**
     * Load the value for args[0]. Returns `Str(_)` on hit, `Null`
     * on miss. The Rust wrapper lifts Null into `Option::None`.
     */
    fun load(args: Array<WhiskerValue>): WhiskerValue {
        if (args.isEmpty()) {
            return WhiskerValue.Err("WhiskerLocalStore.load expects (String,)")
        }
        val key = args[0] as? WhiskerValue.Str
            ?: return WhiskerValue.Err("WhiskerLocalStore.load args[0] must be Str")
        val stored = prefs().getString(key.value, null)
        return if (stored == null) WhiskerValue.Null else WhiskerValue.Str(stored)
    }

    /** Drop args[0]'s entry. Returns Null (→ `()` on the Rust side). */
    fun remove(args: Array<WhiskerValue>): WhiskerValue {
        if (args.isEmpty()) {
            return WhiskerValue.Err("WhiskerLocalStore.remove expects (String,)")
        }
        val key = args[0] as? WhiskerValue.Str
            ?: return WhiskerValue.Err("WhiskerLocalStore.remove args[0] must be Str")
        prefs().edit().remove(key.value).apply()
        return WhiskerValue.Null
    }
}
