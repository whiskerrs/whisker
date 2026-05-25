// Phase L-2a — `ModuleDefinition` DSL surface (Android).
//
// Kotlin counterpart of `platforms/ios/Sources/WhiskerModuleApi/
// ModuleDefinition.swift`. Modeled after Expo Modules'
// `ModuleDefinition` (https://docs.expo.dev/modules/module-api/).
//
// ## Target syntax
//
// ```kotlin
// class VideoModule : WhiskerModule() {
//     override fun definition() = ModuleDefinition {
//         Name("Video")
//
//         Constants("maxResolution" to "1080p")
//
//         View(WhiskerVideoComponent::class.java) {
//             Prop("src") { view: WhiskerVideoComponent, value: String -> view.setSrc(value) }
//             Function("play")  { view: WhiskerVideoComponent -> view.play()  }
//             Function("pause") { view: WhiskerVideoComponent -> view.pause() }
//             Function("seek")  { view: WhiskerVideoComponent, seconds: Double -> view.seek(seconds) }
//             Events("onCompleted")
//         }
//     }
// }
// ```
//
// Function-only modules omit the inner `View(...)` block:
//
// ```kotlin
// class LocalStoreModule : WhiskerModule() {
//     override fun definition() = ModuleDefinition {
//         Name("WhiskerLocalStore")
//         Function("save") { key: String, value: String ->
//             prefs.edit().putString(key, value).apply()
//             true
//         }
//         Function("load") { key: String -> prefs.getString(key, null) }
//     }
// }
// ```
//
// ## What L-2a delivers
//
// Type surface only. The `WhiskerModule` base + DSL types compile
// and authors can write modules using the syntax above; the KSP
// codegen that turns `definition()` into Lynx prop / method
// registrations lands in Phase L-2c. Existing `@WhiskerComponent`
// annotation surface continues in parallel.

package rs.whisker.runtime

// ----- Component model -----------------------------------------------------

/**
 * Type-erased component the DSL collects. Concrete subtypes live
 * below. Authors normally don't reference [WhiskerDefinitionComponent]
 * directly — the factory functions (`Name`, `Prop`, `Function`,
 * etc.) return the right subtype.
 */
public sealed interface WhiskerDefinitionComponent

/** `Name("Foo")` — the module's local tag name. */
public data class WhiskerNameComponent(public val value: String) :
    WhiskerDefinitionComponent

/**
 * `Constants("k" to v, ...)` — static key/value pairs exposed to
 * the host. Dictionary form only in v1; the dynamic closure form
 * and per-key lazy `Constant("k") { ... }` form land later.
 */
public data class WhiskerConstantsComponent(public val values: Map<String, Any?>) :
    WhiskerDefinitionComponent

/**
 * `View(Foo::class.java) { ... }` — registers a Lynx UI subclass
 * + its inner DSL block (Prop / Function / Events). The class is
 * type-erased to `Class<*>` so the parent struct isn't generic;
 * the concrete class is the Lynx UI subclass (typically a
 * [WhiskerUI] subclass).
 */
public data class WhiskerViewComponent(
    public val viewClass: Class<*>,
    public val components: List<WhiskerDefinitionComponent>,
) : WhiskerDefinitionComponent

/**
 * Type-erased prop setter the framework calls on prop dispatch.
 * `view` is the Lynx UI instance; `value` is the raw decoded
 * Lynx value (typed-decoded by L-2c at dispatch time).
 */
public typealias WhiskerPropSetterFn = (view: Any, value: Any?) -> Unit

public data class WhiskerPropComponent(
    public val name: String,
    public val setter: WhiskerPropSetterFn,
) : WhiskerDefinitionComponent

/**
 * Type-erased function handler. `view` is `null` for
 * module-level [Function]s, the Lynx UI instance for view-block
 * [Function]s. `args` carry positional arguments from the JS /
 * Rust call site; the return value is auto-encoded by the
 * dispatch glue (`Unit` → `WhiskerValue.Null`).
 */
public typealias WhiskerFunctionHandlerFn = (view: Any?, args: List<Any?>) -> Any?

public data class WhiskerFunctionComponent(
    public val name: String,
    public val handler: WhiskerFunctionHandlerFn,
) : WhiskerDefinitionComponent

/**
 * `Events("a", "b", ...)` — declare event names this module
 * emits. Just metadata; dispatch stays imperative via
 * `WhiskerCustomEvent.dispatch(...)`.
 */
public data class WhiskerEventsComponent(public val names: List<String>) :
    WhiskerDefinitionComponent

// ----- DSL builders --------------------------------------------------------

/**
 * Top-level builder. Authors call DSL factory functions inside
 * the lambda passed to [ModuleDefinition]; the builder collects
 * the resulting components.
 *
 * Marked `@DslMarker` so the inner [WhiskerViewDefinitionBuilder]
 * doesn't expose top-level factories like [View] that would be
 * nonsensical inside a `View(...) { ... }` block.
 */
@DslMarker
public annotation class WhiskerDefinitionDsl

/**
 * Top-level definition builder. The DSL factories ([Name], [View],
 * [Function], [Constants], [Events]) are **member functions** so
 * authors call them inside the `ModuleDefinition { ... }` block
 * without any `import` — and so `View(...)` doesn't collide with
 * `android.view.View` (a member on the implicit receiver wins over
 * an imported top-level / constructor name).
 *
 * They're plain (non-`inline`/non-`reified`) members; the
 * generic [WhiskerViewDefinitionBuilder.Prop] / `Function`
 * overloads use unchecked casts at dispatch time instead of
 * reified type checks. A type mismatch therefore surfaces as a
 * `ClassCastException` when the closure runs (loud), rather than
 * a silent no-op.
 */
@WhiskerDefinitionDsl
public class WhiskerModuleDefinitionBuilder {
    internal val components: MutableList<WhiskerDefinitionComponent> = mutableListOf()

    /** `Name("Foo")` — the module's local tag name. */
    public fun Name(value: String): WhiskerDefinitionComponent =
        WhiskerNameComponent(value).also { components.add(it) }

    /** `Constants("k" to v, ...)` — static key/value pairs. */
    public fun Constants(vararg entries: Pair<String, Any?>): WhiskerDefinitionComponent =
        WhiskerConstantsComponent(entries.toMap()).also { components.add(it) }

    /** `Constants(mapOf(...))` — same, but takes a Map directly. */
    public fun Constants(values: Map<String, Any?>): WhiskerDefinitionComponent =
        WhiskerConstantsComponent(values).also { components.add(it) }

    /**
     * `View(MyView::class.java) { ... }` — registers a Lynx UI
     * subclass + its inner DSL block (Prop / Function / Events).
     */
    public fun View(
        viewClass: Class<*>,
        block: WhiskerViewDefinitionBuilder.() -> Unit,
    ): WhiskerDefinitionComponent {
        val b = WhiskerViewDefinitionBuilder()
        b.block()
        return WhiskerViewComponent(viewClass, b.components.toList())
            .also { components.add(it) }
    }

    /** `Events("a", "b", ...)` — variadic event-name declaration. */
    public fun Events(vararg names: String): WhiskerDefinitionComponent =
        WhiskerEventsComponent(names.toList()).also { components.add(it) }

    // ---- Module-level (view-less) functions: no view argument ----

    public fun Function(name: String, handler: () -> Any?): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { _, _ -> handler() }.also { components.add(it) }

    public fun <A> Function(
        name: String,
        handler: (A) -> Any?,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { _, args ->
            @Suppress("UNCHECKED_CAST")
            handler(args.getOrNull(0) as A)
        }.also { components.add(it) }

    public fun <A, B> Function(
        name: String,
        handler: (A, B) -> Any?,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { _, args ->
            @Suppress("UNCHECKED_CAST")
            handler(args.getOrNull(0) as A, args.getOrNull(1) as B)
        }.also { components.add(it) }
}

/**
 * Inner builder for the `View(...) { ... }` block. Same
 * member-function rationale as [WhiskerModuleDefinitionBuilder].
 * `@DslMarker` keeps the top-level factories ([Name], [View], …)
 * out of scope here so they can't be called inside a View block.
 */
@WhiskerDefinitionDsl
public class WhiskerViewDefinitionBuilder {
    internal val components: MutableList<WhiskerDefinitionComponent> = mutableListOf()

    /**
     * `Prop("foo") { view, value -> ... }` — view-bearing prop
     * setter. `V` / `T` are inferred from the lambda parameter
     * types. The dispatch-time cast is unchecked (erased generics):
     * a mismatch raises `ClassCastException` inside the closure.
     */
    public fun <V : Any, T> Prop(
        name: String,
        setter: (V, T) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerPropComponent(name) { viewAny, valueAny ->
            @Suppress("UNCHECKED_CAST")
            setter(viewAny as V, valueAny as T)
        }.also { components.add(it) }

    /** `Events("a", "b", ...)` declared inside a `View(...)` block. */
    public fun Events(vararg names: String): WhiskerDefinitionComponent =
        WhiskerEventsComponent(names.toList()).also { components.add(it) }

    // ---- View-bound functions: first arg is the view ----

    public fun <V : Any> Function(
        name: String,
        handler: (V) -> Any?,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { viewAny, _ ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V)
        }.also { components.add(it) }

    public fun <V : Any, A> Function(
        name: String,
        handler: (V, A) -> Any?,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { viewAny, args ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, args.getOrNull(0) as A)
        }.also { components.add(it) }

    public fun <V : Any, A, B> Function(
        name: String,
        handler: (V, A, B) -> Any?,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { viewAny, args ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, args.getOrNull(0) as A, args.getOrNull(1) as B)
        }.also { components.add(it) }
}

// ----- ModuleDefinition value -----------------------------------------------

/**
 * The assembled definition the framework registers with Lynx at
 * module-init time. Immutable; collected from a
 * [WhiskerModuleDefinitionBuilder] block.
 */
public data class ModuleDefinition(public val components: List<WhiskerDefinitionComponent>) {

    /** Module name (first [WhiskerNameComponent] in the components list). */
    public val name: String?
        get() = components.firstNotNullOfOrNull { (it as? WhiskerNameComponent)?.value }

    /** View block, if any. */
    public val view: WhiskerViewComponent?
        get() = components.firstNotNullOfOrNull { it as? WhiskerViewComponent }

    /** Merged constants from all [WhiskerConstantsComponent] blocks. */
    public val constants: Map<String, Any?>
        get() = buildMap {
            for (c in components) {
                if (c is WhiskerConstantsComponent) putAll(c.values)
            }
        }

    /** Module-level (view-less) [Function] declarations. */
    public val functions: List<WhiskerFunctionComponent>
        get() = components.filterIsInstance<WhiskerFunctionComponent>()

    public companion object {
        /**
         * Builder-style constructor — used as
         * `ModuleDefinition { Name(...); Function(...) { ... } }`.
         */
        public operator fun invoke(
            block: WhiskerModuleDefinitionBuilder.() -> Unit,
        ): ModuleDefinition {
            val b = WhiskerModuleDefinitionBuilder()
            b.block()
            return ModuleDefinition(b.components.toList())
        }
    }
}
