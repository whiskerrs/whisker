package rs.whisker.annotations

/**
 * Marks a `@WhiskerElement`-annotated class's method as a UI
 * method invokable from Rust via an `ElementRef<T>`. Phase 7-Φ.H.2.
 *
 * Author writes:
 *
 * ```kotlin
 * @WhiskerElement("Video")
 * open class WhiskerVideoElement(context: WhiskerContext) :
 *     WhiskerUI<View>(context) {
 *
 *     override fun createView(context: Context): View = VideoView(context)
 *
 *     @WhiskerUIMethod
 *     open fun play(args: List<WhiskerValue>): WhiskerValue {
 *         (view as? VideoView)?.start()
 *         return WhiskerValue.Null
 *     }
 * }
 * ```
 *
 * The `:ksp` companion processor adds an `@LynxUIMethod`-tagged
 * forwarder onto the `<Class>_LynxBridge` subclass it already
 * generates for `@WhiskerProp`. Lynx's `LynxUIMethodsExecutor`
 * reflection-dispatches to that forwarder, which decodes the
 * incoming `ReadableMap` back into a `List<WhiskerValue>` and
 * invokes the original `open` method through Kotlin (so the user's
 * method body runs).
 *
 * Why a separate annotation instead of typealiasing `@LynxUIMethod`:
 * Kotlin's `typealias` does not support annotation types — same
 * constraint that drove `@WhiskerProp`. The KSP-generated bridge
 * subclass is the workaround.
 *
 * The annotated method MUST be `open` so the bridge subclass can
 * forward through it via `super.<methodName>(args)`.
 *
 * Method shape contract (matches `@WhiskerModule`):
 *   - one parameter of type `List<WhiskerValue>`
 *   - return type `WhiskerValue`
 *   - synchronous (v1; async callbacks tracked separately)
 *
 * The `WhiskerUIMethod` annotation reserves namespace `WhiskerUI*`
 * for element-side method declarations; `WhiskerMethod` is reserved
 * for future module-side method declarations.
 *
 * `SOURCE` retention — the annotation has no runtime role once
 * the bridge code is generated.
 */
@Target(AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerUIMethod
