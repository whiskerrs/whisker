package rs.whisker.annotations

/**
 * Marks a class as the platform-side implementation of a Whisker
 * platform module registered under [name].
 *
 * Companion of iOS's `@WhiskerModule` Swift Macro (under
 * `platforms/ios/macros`). Authors apply this annotation
 * to a Kotlin class that exposes one or more methods with the
 * single-`Array<Any?>` argument shape `whisker_bridge_invoke_module`
 * expects:
 *
 * ```kotlin
 * @WhiskerModule("WhiskerStorage")
 * class WhiskerStorageImpl(private val context: Context) {
 *     fun save(args: Array<Any?>): Any {
 *         val key = args[0] as String
 *         val value = args[1] as String
 *         context.getSharedPreferences("whisker", Context.MODE_PRIVATE)
 *             .edit().putString(key, value).apply()
 *         return true
 *     }
 * }
 * ```
 *
 * The `:ksp` companion processor scans the user app's compilation
 * for every `@WhiskerModule(...)`-annotated class and folds a
 * `WhiskerModuleRegistry.registerModuleClass(name, MyClass::class.java)`
 * call into `rs.whisker.runtime.generated.WhiskerModuleBehaviors.
 * registerAll()` — sibling registration to the `@WhiskerComponent`
 * processing.
 *
 * Pairs with the Rust-side `#[whisker::platform_module]` proc macro
 * (Phase 7-Φ.E.5) — the Kotlin class is the platform implementer
 * the Rust proxy ultimately dispatches against via JNI.
 *
 * `SOURCE` retention because KSP only reads annotations at
 * compile time — once the registration is generated, the
 * annotation has no runtime role and shouldn't bloat the APK.
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerModule(val name: String)
