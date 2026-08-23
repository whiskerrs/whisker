package rs.whisker.runtime

import android.app.Application
import android.content.Context

/**
 * Base Application class for Whisker apps.
 *
 * Keeps a process-wide application [Context] for view-less modules.
 * Generated apps do not need to subclass this type: `WhiskerView`
 * calls [initialize] when it is constructed. The subclass remains useful
 * for applications that need the context before their first view exists.
 */
open class WhiskerApplication : Application() {
    public companion object {
        /**
         * The ApplicationContext, set in [onCreate]. `Module`
         * subclasses reach this lazily because the bridge
         * instantiates them with a zero-arg ctor — there's no Context
         * to inject at construction time.
         *
         * Reading from arbitrary background threads is safe; the
         * value is a stable per-process reference, written once
         * before any module dispatch happens.
         */
        @JvmStatic
        @Volatile
        public var appContext: Context? = null
            private set

        /** Install the application context without requiring an Application subclass. */
        @JvmStatic
        public fun initialize(context: Context) {
            if (appContext == null) appContext = context.applicationContext
        }
    }

    override fun onCreate() {
        super.onCreate()
        initialize(this)
    }
}
