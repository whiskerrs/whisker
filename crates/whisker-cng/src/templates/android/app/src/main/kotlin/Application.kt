package {{android_application_id}}

import rs.whisker.runtime.WhiskerApplication

class {{android_application_class}} : WhiskerApplication() {
    override fun onCreate() {
        // Load bridge first (it pulls in liblynx via DT_NEEDED). Then
        // load the Rust dylib, whose undef references to
        // `whisker_bridge_*` resolve against the already-loaded bridge.
        // The bridge's own `dlsym` of `whisker_app_main` runs lazily
        // later, by which time the Rust lib is in `RTLD_DEFAULT`.
        super.onCreate()
        System.loadLibrary("{{rust_lib_name}}")
    }
}
