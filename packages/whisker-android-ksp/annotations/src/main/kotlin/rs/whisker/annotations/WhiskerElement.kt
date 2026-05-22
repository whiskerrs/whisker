package rs.whisker.annotations

/**
 * Marks a Lynx `LynxUI` subclass as the implementation of a Whisker
 * native element with the given [tag].
 *
 * Companion of iOS's `@WhiskerElement` Swift Macro (under
 * `packages/whisker-ios-macros`). Module-crate authors apply this
 * annotation to their `LynxUI<View>` subclass:
 *
 * ```kotlin
 * @WhiskerElement("x-hello")
 * class WhiskerHelloElement(context: LynxContext) : LynxUI<View>(context) {
 *     override fun createView(context: Context): View {
 *         val v = View(context)
 *         v.setBackgroundColor(Color.YELLOW)
 *         return v
 *     }
 * }
 * ```
 *
 * The `:ksp` companion processor scans the user app's compilation
 * for every `@WhiskerElement(...)`-annotated class and generates a
 * single `rs.whisker.runtime.generated.WhiskerModuleBehaviors`
 * Kotlin object whose `registerAll()` calls
 * `LynxEnv.inst().addBehavior(...)` once per declared `(tag,
 * class)`. The generated object is invoked from the user app's
 * `Application.onCreate()` (template-rendered by
 * `whisker-cng/src/templates/android/app/src/main/kotlin/
 * Application.kt`) before any `WhiskerView` is constructed.
 *
 * `SOURCE` retention because KSP only reads annotations at
 * compile time — once the registration is generated, the
 * annotation has no runtime role and shouldn't bloat the APK.
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerElement(val tag: String)
