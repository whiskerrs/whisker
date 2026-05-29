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
}
