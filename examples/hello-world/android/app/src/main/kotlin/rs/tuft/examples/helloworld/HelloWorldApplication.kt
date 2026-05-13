package rs.tuft.examples.helloworld

import rs.tuft.runtime.TuftApplication

class HelloWorldApplication : TuftApplication() {
    override fun onCreate() {
        // Load bridge first (it pulls in liblynx via DT_NEEDED). Then
        // load the Rust cdylib, whose undef references to `tuft_bridge_*`
        // resolve against the already-loaded bridge .so. The bridge's
        // own `dlsym` of `tuft_mobile_app_main` runs lazily later, by
        // which time the Rust lib is in `RTLD_DEFAULT`.
        super.onCreate()
        System.loadLibrary("hello_world")
    }
}
