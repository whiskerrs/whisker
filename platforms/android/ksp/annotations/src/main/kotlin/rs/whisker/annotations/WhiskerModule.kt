package rs.whisker.annotations

/**
 * Marks a [rs.whisker.runtime.Module] subclass as a Whisker module
 * the KSP processor should register.
 *
 * Applying `@WhiskerModule` is the registration trigger — the
 * `:ksp` companion processor scans the user app's compilation for
 * every `@WhiskerModule`-annotated class, reads its
 * `definitionLazy`, and folds the Lynx behaviour / module-dispatch
 * registration into `rs.whisker.runtime.generated.<Module>Behaviors
 * .registerAll()`. The module's local tag / name comes from the
 * `Name("…")` entry inside `definition()`, so the annotation itself
 * takes no arguments.
 *
 * ```kotlin
 * import rs.whisker.annotations.WhiskerModule
 * import rs.whisker.runtime.Module
 *
 * @WhiskerModule
 * class VideoModule : Module() {
 *     override fun definition() = ModuleDefinition {
 *         Name("Video")
 *         View(VideoView::class.java) {
 *             Prop("src") { view: VideoView, value: String -> view.setSrc(value) }
 *             Function("play") { view: VideoView -> view.play() }
 *         }
 *     }
 * }
 * ```
 *
 * Companion of iOS's `@WhiskerModule` Swift macro (under
 * `platforms/ios/macros`). `SOURCE` retention — KSP only reads it
 * at compile time; it has no runtime role.
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerModule
