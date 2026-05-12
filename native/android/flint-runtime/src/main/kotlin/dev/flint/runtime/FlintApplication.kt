package dev.flint.runtime

import android.app.Application
import com.lynx.tasm.LynxEnv

/**
 * Base Application class for Flint apps.
 *
 * The CNG-generated `MainApplication` extends this. Loads the native libraries
 * (Rust runtime + bridge + Lynx) and initializes LynxEnv.
 *
 * Plugin authors that need Application-level initialization should declare a
 * `plugin-app-init` injection in their `flint_plugin` function and the codegen
 * will splice it into MainApplication.onCreate().
 */
open class FlintApplication : Application() {
    override fun onCreate() {
        super.onCreate()

        // Order matters: bridge depends on Lynx symbols.
        System.loadLibrary("lynx")
        System.loadLibrary("flint_bridge")
        System.loadLibrary("flint_runtime")

        LynxEnv.inst().init(
            /* application = */ this,
            /* libraryLoader = */ null,
            /* templateProvider = */ null,
            /* behaviorBundle = */ null,
        )

        nativeInitRuntime()
    }

    private external fun nativeInitRuntime()
}
