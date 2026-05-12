package dev.lyra.runtime

import android.app.Application
import com.lynx.tasm.LynxEnv

/**
 * Base Application class for Lyra apps.
 *
 * Pulls in libc++_shared.so explicitly before the Lynx libs so its
 * `__cxa_*` / `_Unwind_*` symbols are available — our Lynx build links
 * libc++abi statically, leaves those as undefined externs, and its
 * DT_NEEDED list doesn't include libc++_shared.so. After that, `LynxEnv`
 * is initialised once per process.
 *
 * The user app's own `Application` subclass is responsible for loading
 * its Rust cdylib (the one produced from the `#[lyra::main]` crate)
 * before `super.onCreate()` — the bridge resolves
 * `lyra_mobile_app_main` from `RTLD_DEFAULT`, which only sees libraries
 * the process has already loaded.
 */
open class LyraApplication : Application() {
    override fun onCreate() {
        super.onCreate()
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
