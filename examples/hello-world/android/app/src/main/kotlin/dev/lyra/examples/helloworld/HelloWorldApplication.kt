package dev.lyra.examples.helloworld

import dev.lyra.runtime.LyraApplication

class HelloWorldApplication : LyraApplication() {
    override fun onCreate() {
        // Load bridge first (it pulls in liblynx via DT_NEEDED). Then
        // load the Rust cdylib, whose undef references to `lyra_bridge_*`
        // resolve against the already-loaded bridge .so. The bridge's
        // own `dlsym` of `lyra_mobile_app_main` runs lazily later, by
        // which time the Rust lib is in `RTLD_DEFAULT`.
        super.onCreate()
        System.loadLibrary("hello_world")
    }
}
