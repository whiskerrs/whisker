// `Module` base class (Android) — the API a Whisker module
// subclasses. Mark the subclass with `@WhiskerModule` (from
// `rs.whisker.annotations`) so the KSP processor discovers it and
// generates the Lynx registration. (Modeled after Expo's `Module`
// base class; the `@WhiskerModule` marker plays the role of Expo's
// `expo-module.config.json` entry — but inline at the declaration.)
//
// ```kotlin
// import rs.whisker.annotations.WhiskerModule
// import rs.whisker.runtime.Module        // ← explicit import: bare
//                                         //   `Module` would resolve
//                                         //   to java.lang.Module
//
// @WhiskerModule
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
