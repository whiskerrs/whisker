package rs.whisker.annotations

/**
 * Marks a `WhiskerUI` subclass as the implementation of a Whisker
 * native element with the given local [tag].
 *
 * Companion of iOS's `@WhiskerComponent` Swift Macro (under
 * `packages/whisker-ios-macros`). Module-crate authors apply this
 * annotation to their `WhiskerUI<View>` subclass:
 *
 * ```kotlin
 * @WhiskerComponent("Hello")
 * class WhiskerHelloComponent(context: WhiskerContext) : WhiskerUI<View>(context) {
 *     override fun createView(context: Context): View {
 *         val v = View(context)
 *         v.setBackgroundColor(Color.YELLOW)
 *         return v
 *     }
 * }
 * ```
 *
 * The [tag] is the *local* name. At codegen time the KSP processor
 * prepends the cargo crate name (passed via Gradle's
 * `ksp { arg("whisker.crateName", "<crate>") }` in each module's
 * `build.gradle.kts`) to produce the fully-qualified registration
 * string `<crate-name>:<local-tag>` — so two unrelated module
 * packages can both declare a `Hello` element without colliding
 * in Lynx's behaviour registry. Phase 7-Φ.H.2.
 *
 * The processor scans each subproject's compilation, generates a
 * per-module `<ModuleName>Behaviors` Kotlin object whose
 * `register(env: WhiskerEnv)` calls `env.addBehavior(...)` once per
 * declared `(qualifiedTag, class)`, and the whisker-build-generated
 * aggregator in the user app calls every per-module register fn
 * from its top-level `WhiskerModuleBehaviors.registerAll(env)`.
 *
 * `SOURCE` retention because KSP only reads annotations at compile
 * time — once the registration is generated, the annotation has
 * no runtime role and shouldn't bloat the APK.
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.SOURCE)
@MustBeDocumented
public annotation class WhiskerComponent(val tag: String)
