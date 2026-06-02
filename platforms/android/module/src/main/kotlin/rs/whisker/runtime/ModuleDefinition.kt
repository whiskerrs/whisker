// Phase L-2a — `ModuleDefinition` DSL surface (Android).
//
// Kotlin counterpart of `platforms/ios/Sources/WhiskerModule/
// ModuleDefinition.swift`. Modeled after Expo Modules'
// `ModuleDefinition` (https://docs.expo.dev/modules/module-api/).
//
// ## Target syntax
//
// ```kotlin
// class VideoModule : Module() {
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
// class LocalStoreModule : Module() {
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
// Type surface only. The `Module` base + DSL types compile and
// authors write modules using the syntax above; the KSP codegen
// (L-2c) walks every `Module` subclass and turns `definition()`
// into Lynx prop / method registrations.

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
 * `view` is the Lynx UI instance; `value` is the raw
 * [WhiskerValue] (case ②: no auto-deserialization — the author
 * destructures it, e.g. `value.asString()`).
 */
public typealias WhiskerPropSetterFn = (view: Any, value: WhiskerValue) -> Unit

public data class WhiskerPropComponent(
    public val name: String,
    public val setter: WhiskerPropSetterFn,
) : WhiskerDefinitionComponent

/**
 * Type-erased function handler. `view` is `null` for module-level
 * [Function]s, the Lynx UI instance for view-block [Function]s.
 * `args` are the raw positional [WhiskerValue]s from the Rust call
 * site (case ②: no auto-deserialization — the author destructures,
 * e.g. `args[0].asDouble()`); the return is a raw [WhiskerValue]
 * (`WhiskerValue.Null` for "no result").
 */
public typealias WhiskerFunctionHandlerFn = (view: Any?, args: List<WhiskerValue>) -> WhiskerValue

public data class WhiskerFunctionComponent(
    public val name: String,
    public val handler: WhiskerFunctionHandlerFn,
) : WhiskerDefinitionComponent

/**
 * `Events("a", "b", ...)` — declare event names this module emits.
 * Metadata only; dispatch goes through
 * [Module.sendEvent], which fans the payload out to every Rust
 * subscriber registered via `PlatformModule::on_event`.
 */
public data class WhiskerEventsComponent(public val names: List<String>) :
    WhiskerDefinitionComponent

/**
 * `OnStartObserving("name") { ... }` — fires when the listener
 * count for `(this module, "name")` transitions from 0 to 1. Use to
 * lazily attach an expensive source (e.g. `OnBackInvokedCallback`
 * registration, sensor open) so the work only runs while at least
 * one Rust subscriber is active.
 */
public data class WhiskerOnStartObservingComponent(
    public val eventName: String,
    public val handler: () -> Unit,
) : WhiskerDefinitionComponent

/**
 * `OnStopObserving("name") { ... }` — fires when the listener count
 * for `(this module, "name")` transitions from 1 to 0. Tears down
 * whatever `OnStartObserving` set up.
 */
public data class WhiskerOnStopObservingComponent(
    public val eventName: String,
    public val handler: () -> Unit,
) : WhiskerDefinitionComponent

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

    /**
     * `OnStartObserving("name") { ... }` — declare a lazy-start
     * hook for `name`. The closure fires once on the 0→1
     * listener-count transition for `(this module, "name")`.
     */
    public fun OnStartObserving(
        name: String,
        handler: () -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerOnStartObservingComponent(name, handler).also { components.add(it) }

    /**
     * `OnStopObserving("name") { ... }` — pair to
     * `OnStartObserving`. Fires on the 1→0 transition.
     */
    public fun OnStopObserving(
        name: String,
        handler: () -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerOnStopObservingComponent(name, handler).also { components.add(it) }

    // ---- Module-level (view-less) function: raw args (case ②) ----

    /**
     * `Function("save") { args -> WhiskerValue.Bool(...) }` — the
     * author reads `args[i]` (e.g. `args[0].asString()`) and returns
     * a [WhiskerValue]. No arity overloads, no type coercion.
     */
    public fun Function(
        name: String,
        handler: (args: List<WhiskerValue>) -> WhiskerValue,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { _, args -> handler(args) }.also { components.add(it) }
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
     * `Prop("src") { view: VideoView, value -> view.setSrc(value.asString()) }`
     * — view-bearing prop setter. Case ②: `value` is the raw
     * [WhiskerValue]; the author destructures it. `V` is inferred
     * from the lambda; the dispatch-time view cast is unchecked
     * (erased generics) — a mismatch raises `ClassCastException`.
     */
    public fun <V : Any> Prop(
        name: String,
        setter: (V, WhiskerValue) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerPropComponent(name) { viewAny, value ->
            @Suppress("UNCHECKED_CAST")
            setter(viewAny as V, value)
        }.also { components.add(it) }

    /** `Events("a", "b", ...)` declared inside a `View(...)` block. */
    public fun Events(vararg names: String): WhiskerDefinitionComponent =
        WhiskerEventsComponent(names.toList()).also { components.add(it) }

    // ---- View-bound function: view + raw args (case ②) ----

    /**
     * `Function("seek") { view: VideoView, args -> view.seek(args[0].asDouble()); WhiskerValue.Null }`
     * — the author reads `args[i]` and returns a [WhiskerValue].
     */
    public fun <V : Any> Function(
        name: String,
        handler: (view: V, args: List<WhiskerValue>) -> WhiskerValue,
    ): WhiskerDefinitionComponent =
        WhiskerFunctionComponent(name) { viewAny, args ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, args)
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

    /** Module-level [OnStartObserving] hooks. */
    public val onStartObservingHooks: List<WhiskerOnStartObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStartObservingComponent>()

    /** Module-level [OnStopObserving] hooks. */
    public val onStopObservingHooks: List<WhiskerOnStopObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStopObservingComponent>()

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
