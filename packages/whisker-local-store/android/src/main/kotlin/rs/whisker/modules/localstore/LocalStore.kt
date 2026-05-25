// SharedPreferences-backed persistent string store. App-private
// (named "WhiskerLocalStore"), persists across launches, doesn't
// sync across devices.
//
// Plain helper — no Whisker / Lynx types. The DSL module that
// exposes it to Rust lives in `LocalStoreModule.kt`.

package rs.whisker.modules.localstore

import android.content.Context
import android.content.SharedPreferences
import rs.whisker.runtime.WhiskerApplication

internal object LocalStore {
    private const val PREFS_NAME = "WhiskerLocalStore"

    /**
     * Resolve the app-private SharedPreferences. Lazy via
     * `WhiskerApplication.appContext`, which the host app's
     * `Application.onCreate()` installs before any module dispatch
     * could reach here.
     */
    private fun prefs(): SharedPreferences {
        val ctx = WhiskerApplication.appContext
            ?: throw IllegalStateException(
                "WhiskerLocalStore: WhiskerApplication.appContext not initialised — " +
                    "ensure your Application extends WhiskerApplication and " +
                    "super.onCreate() runs before any module dispatch",
            )
        return ctx.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    /** Persist [value] under [key]. Returns `true` on commit success. */
    fun save(key: String, value: String): Boolean =
        prefs().edit().putString(key, value).commit()

    /** Read [key]; `null` on miss (→ `Option::None` on the Rust side). */
    fun load(key: String): String? = prefs().getString(key, null)

    /** Drop [key]'s entry. */
    fun remove(key: String) {
        prefs().edit().remove(key).apply()
    }
}
