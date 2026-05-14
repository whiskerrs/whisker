package rs.whisker.examples.helloworld

import rs.whisker.runtime.WhiskerApplication

class HelloWorldApplication : WhiskerApplication() {
    override fun onCreate() {
        // Load bridge first (it pulls in liblynx via DT_NEEDED). Then
        // load the Rust cdylib, whose undef references to `whisker_bridge_*`
        // resolve against the already-loaded bridge .so. The bridge's
        // own `dlsym` of `whisker_app_main` runs lazily later, by which time
        // the Rust lib is in `RTLD_DEFAULT`.
        super.onCreate()
        System.loadLibrary("hello_world")
    }
}
