package rs.whisker.annotations

/**
 * Marks a `@WhiskerComponent`-annotated class's method as a prop
 * setter that should be wired up to Lynx's attribute-dispatch
 * machinery.
 *
 * Author writes:
 *
 * ```kotlin
 * @WhiskerComponent("x-input")
 * open class WhiskerInput(context: WhiskerContext) : WhiskerUI<EditText>(context) {
 *     override fun createView(context: Context): EditText = EditText(context)
 *
 *     @WhiskerProp("value")
 *     open fun setValue(value: String) {
 *         view?.setText(value)
 *     }
 * }
 * ```
 *
 * The `:ksp` companion processor scans `@WhiskerComponent`-annotated
 * classes for `@WhiskerProp`-tagged methods and generates a
 * `<Class>_LynxBridge` subclass alongside the per-module behaviors
 * file. The bridge subclass declares matching `@LynxProp(name =
 * ...)` wrapper methods that forward to the original, then the
 * generated `Behaviors.registerAll()` registers the BRIDGE class
 * with Lynx instead of the user class. Lynx's reflection-based
 * prop dispatch finds the `@LynxProp`-tagged wrappers on the
 * bridge and Whisker module authors never have to mention Lynx
 * symbols in their own code.
 *
 * Why a separate annotation instead of typealiasing `@LynxProp`:
 * Kotlin's `typealias` keyword does not support annotation types
 * (the compiler refuses `typealias WhiskerProp = LynxProp`). The
 * KSP-generated subclass is the next-cleanest workaround.
 *
 * The annotated method MUST be `open` so the bridge subclass can
 * forward through it. (Lynx's own `@LynxProp` does not require
 * `open` because reflection invokes the method directly, but our
 * bridge needs Kotlin-level call-through.)
 *
 * `SOURCE` retention — the annotation has no runtime role once
 * the bridge code is generated.
 */
@Target(AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerProp(val name: String)
