// `Module` base class (Android) — the API a Whisker module
// subclasses. **Subclassing is the registration signal** — the
// KSP processor (`rs.whisker.ksp.WhiskerModuleProcessor`) walks
// every concrete subclass and emits the Lynx registration. No
// marker annotation is required at the declaration site.
//
// ```kotlin
// import rs.whisker.runtime.Module        // ← explicit import: bare
//                                         //   `Module` would resolve
//                                         //   to java.lang.Module
//
// class VideoModule : Module() {
//     override fun definition() = ModuleDefinition {
//         Name("Video")
//         View(VideoView::class.java) {
//             Prop("src") { view: VideoView, value: String -> view.setSrc(value) }
//             Function("play") { view: VideoView -> view.play() }
//         }
//     }
// }
// ```

package rs.whisker.runtime

public abstract class Module {
    /**
     * Authors override to declare the module via the DSL. Default
     * impl returns an empty definition — useful for tests and as a
     * sentinel.
     */
    public open fun definition(): ModuleDefinition = ModuleDefinition(emptyList())

    /**
     * Cached [definition] value — computed lazily on first access so
     * subclasses can do expensive setup in `definition()` without
     * paying for it on every call.
     */
    public val definitionLazy: ModuleDefinition by lazy { definition() }

    /**
     * Fully-qualified module name (`<crate>:<Name>`), set by the
     * KSP-generated registration call. `null` until registered —
     * `sendEvent` silently no-ops in that window. Authors must NOT
     * set this themselves; the KSP processor does. (Public-set
     * rather than `internal set` because the generated
     * `<Module>Behaviors.kt` lives in the consumer Gradle module,
     * not in `rs.whisker.runtime`, so `internal` is out of reach.)
     */
    public var qualifiedName: String? = null

    /**
     * Dispatch [payload] to every Rust subscriber of [event] on this
     * module. The bridge fans the call out to every
     * `PlatformModule::on_event` callback registered against
     * `(this.qualifiedName, event)`.
     *
     * Defaults to [WhiskerValue.Null] for an unparameterised ping.
     * No-op if the module hasn't been registered yet.
     */
    public fun sendEvent(event: String, payload: WhiskerValue = WhiskerValue.Null) {
        val qname = qualifiedName ?: return
        WhiskerModuleEventCenter.dispatchSend(qname, event, payload)
    }

    /**
     * Fire every `OnStartObserving("eventName")` hook on this
     * module. Called by [WhiskerModuleEventCenter] when the JNI
     * trampoline routes a bridge start event back here. Public so
     * the center (a different file) can dispatch into it; authors
     * should not call this directly.
     */
    public fun fireOnStartObserving(eventName: String) {
        for (h in definitionLazy.onStartObservingHooks) {
            if (h.eventName == eventName) h.handler()
        }
    }

    /** Counterpart to [fireOnStartObserving]. */
    public fun fireOnStopObserving(eventName: String) {
        for (h in definitionLazy.onStopObservingHooks) {
            if (h.eventName == eventName) h.handler()
        }
    }
}
