// Phase L-2a — `WhiskerModule` base class (Android).
//
// Authors subclass `WhiskerModule` and override `definition()` to
// declare their module via the DSL builder in `ModuleDefinition.kt`.
// The DSL surface is fully usable (compiles, type-checks, runs) as
// of L-2a; the KSP codegen that translates `definition()` into
// Lynx prop / method registrations lands in Phase L-2c.
//
// The existing `@WhiskerComponent` / `@WhiskerProp` /
// `@WhiskerUIMethod` annotation surface continues to drive real
// registrations in parallel — both paths coexist through Phase
// L-3, and the annotation surface gets deprecated in Phase M.

package rs.whisker.runtime

/**
 * Base class for Whisker modules.
 *
 * ```kotlin
 * class VideoModule : WhiskerModule() {
 *     override fun definition() = ModuleDefinition {
 *         Name("Video")
 *         View(WhiskerVideoComponent::class.java) {
 *             Prop("src") { view: WhiskerVideoComponent, value: String -> view.setSrc(value) }
 *             Function("play") { view: WhiskerVideoComponent -> view.play() }
 *         }
 *     }
 * }
 * ```
 *
 * Phase L-2a: the base class exposes a [definitionLazy] property
 * that computes the definition on first access (single-threaded
 * at module registration time) and caches it. Phase L-2c's KSP
 * processor + bootstrap code reads it to drive Lynx registrations.
 */
public abstract class WhiskerModule {
    /**
     * Authors override to declare the module via the DSL.
     * Default impl returns an empty definition — useful for tests
     * and as a sentinel during the L-2a → L-2c migration.
     */
    public open fun definition(): ModuleDefinition = ModuleDefinition(emptyList())

    /**
     * Cached [definition] value. Computed lazily on first access
     * so subclasses can perform expensive setup inside
     * `definition()` without paying for it on every call.
     */
    public val definitionLazy: ModuleDefinition by lazy { definition() }
}
