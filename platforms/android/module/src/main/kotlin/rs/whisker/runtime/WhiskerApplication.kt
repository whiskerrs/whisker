package rs.whisker.runtime

import android.app.Application
import android.content.Context
import com.lynx.tasm.LynxEnv

/**
 * Base Application class for Whisker apps.
 *
 * Pulls in libc++_shared.so explicitly before the Lynx libs so its
 * `__cxa_*` / `_Unwind_*` symbols are available — our Lynx build links
 * libc++abi statically, leaves those as undefined externs, and its
 * DT_NEEDED list doesn't include libc++_shared.so. After that, `LynxEnv`
 * is initialised once per process.
 *
 * The user app's own `Application` subclass is responsible for loading
 * its Rust cdylib (the one produced from the `#[whisker::main]` crate)
 * before `super.onCreate()` — the bridge resolves `whisker_app_main` from
 * `RTLD_DEFAULT`, which only sees libraries the process has already
 * loaded.
 *
 * Also stashes a process-wide [appContext] reference for Whisker
 * platform modules instantiated reflectively by the C bridge (their
 * `cls.getDeclaredConstructor().newInstance()` zero-arg ctor has no
 * Context to pull from). Modules read [appContext] lazily on first
 * dispatch — by that time `onCreate` has long since run.
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
    }

    override fun onCreate() {
        super.onCreate()
        appContext = applicationContext
        for (name in listOf("c++_shared", "quick", "lynxbase", "lynxtrace", "lynx")) {
            System.loadLibrary(name)
        }
        LynxEnv.inst().init(
            /* application = */ this,
            /* libraryLoader = */ null,
            /* templateProvider = */ null,
            /* behaviorBundle = */ null,
        )
    }
}
