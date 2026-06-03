package rs.whisker.svgexample

import rs.whisker.runtime.WhiskerApplication

class WhiskerSvgExampleApplication : WhiskerApplication() {
    override fun onCreate() {
        // Load bridge first (it pulls in liblynx via DT_NEEDED). Then
        // load the Rust dylib, whose undef references to
        // `whisker_bridge_*` resolve against the already-loaded bridge.
        // The bridge's own `dlsym` of `whisker_app_main` runs lazily
        // later, by which time the Rust lib is in `RTLD_DEFAULT`.
        super.onCreate()
        System.loadLibrary("whisker_svg_example")

        // Phase 7-Φ.C: register every Whisker module's
        // `[android].behaviors` entry against Lynx's behaviour
        // registry. The generated `WhiskerModuleBehaviors` object
        // lives in this app module (`whisker_modules/` source set)
        // so the call has to happen here — the runtime module
        // (`WhiskerView`'s home) doesn't have access to the
        // generated symbol. Must run before the first `WhiskerView`
        // construction so `create_element_by_name(...)` calls
        // issued during `nativeAppMain` find their class.
        rs.whisker.runtime.generated.WhiskerModuleBehaviors.registerAll()
    }
}
