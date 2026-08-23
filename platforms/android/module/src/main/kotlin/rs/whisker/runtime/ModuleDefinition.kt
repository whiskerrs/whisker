// `ModuleDefinition` DSL surface (Android).
//
// Kotlin counterpart of `platforms/ios/Sources/WhiskerModule/
// ModuleDefinition.swift`. Modeled after Expo Modules'
// `ModuleDefinition` (https://docs.expo.dev/modules/module-api/).
//
// ## Syntax
//
// ```kotlin
// @WhiskerModule
// class VideoModule : Module() {
//     override fun definition() = ModuleDefinition {
//         Name("Video")
//
//         Constants("maxResolution" to WhiskerValue.Str("1080p"))
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
// @WhiskerModule
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
// The KSP codegen walks every `@WhiskerModule` declaration and turns
// `definition()` into Lynx prop / method registrations.

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
 * the host. Dictionary form only.
 */
public data class WhiskerConstantsComponent(public val values: Map<String, WhiskerValue>) :
    WhiskerDefinitionComponent

/**
 * `View(Foo::class.java) { ... }` — registers a Lynx UI subclass
 * + its inner DSL block (Prop / Function / Events). The class is
 * type-erased to `Class<*>` so the parent struct isn't generic;
 * the concrete class is the Lynx UI subclass (typically a
 * [WhiskerUI] subclass).
 */
public data class WhiskerViewComponent(
    public val viewClass: Class<*>? = null,
    public val components: List<WhiskerDefinitionComponent>,
    public val elementName: String? = null,
    internal val factory: WhiskerElementFactory? = null,
) : WhiskerDefinitionComponent

/**
 * Type-erased prop setter the framework calls on prop dispatch.
 * `view` is the Lynx UI instance; `value` is the raw
 * [WhiskerValue] — no auto-deserialization, the author destructures
 * it, e.g. `value.asString()`.
 */
public typealias WhiskerPropSetterFn = (view: Any, value: WhiskerValue) -> Unit
public typealias WhiskerPropClearerFn = (view: Any) -> Unit

public data class WhiskerPropComponent(
    public val name: String,
    public val setter: WhiskerPropSetterFn,
    public val clearer: WhiskerPropClearerFn,
) : WhiskerDefinitionComponent

/**
 * Type-erased function handler. `view` is `null` for module-level
 * [Function]s, the Lynx UI instance for view-block [Function]s.
 * `args` are the raw positional [WhiskerValue]s from the Rust call
 * site — no auto-deserialization, the author destructures, e.g.
 * `args[0].asDouble()`; the return is a raw [WhiskerValue]
 * (`WhiskerValue.Null` for "no result").
 */
public typealias WhiskerFunctionHandlerFn = (view: Any?, args: List<WhiskerValue>) -> WhiskerValue

public data class WhiskerFunctionComponent(
    public val name: String,
    public val handler: WhiskerFunctionHandlerFn,
) : WhiskerDefinitionComponent

/**
 * Type-erased ASYNC function handler. Like [WhiskerFunctionHandlerFn]
 * but instead of returning a value it resolves the given [promise] —
 * now or later, e.g. from a purchase / network completion. `view` is
 * `null` for module-level `AsyncFunction`s. Mirrors Expo's
 * `AsyncFunction` + `Promise` (callback-resolved form).
 */
public typealias WhiskerAsyncFunctionHandlerFn =
    (view: Any?, args: List<WhiskerValue>, promise: WhiskerPromise) -> Unit

public data class WhiskerAsyncFunctionComponent(
    public val name: String,
    public val handler: WhiskerAsyncFunctionHandlerFn,
) : WhiskerDefinitionComponent

/**
 * `Events("a", "b", ...)` — declare event names this module emits.
 * Metadata only; dispatch goes through
 * [Module.sendEvent], which fans the payload out to every Rust
 * subscriber registered via `PlatformModule::on_event`.
 */
public data class WhiskerEventsComponent(
    public val names: List<String>,
) :
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
    public fun Constants(vararg entries: Pair<String, WhiskerValue>): WhiskerDefinitionComponent =
        WhiskerConstantsComponent(entries.toMap()).also { components.add(it) }

    /** `Constants(mapOf(...))` — same, but takes a Map directly. */
    public fun Constants(values: Map<String, WhiskerValue>): WhiskerDefinitionComponent =
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
        return WhiskerViewComponent(viewClass = viewClass, components = b.components.toList())
            .also { components.add(it) }
    }

    /** String-declared element identity, resolved to Rust IDs at bootstrap. */
    public fun View(
        elementName: String,
        viewClass: Class<*>,
        block: WhiskerViewDefinitionBuilder.() -> Unit,
    ): WhiskerDefinitionComponent {
        val b = WhiskerViewDefinitionBuilder()
        b.block()
        return WhiskerViewComponent(viewClass = viewClass, components = b.components.toList(), elementName = elementName)
            .also { components.add(it) }
    }

    /** Registers a Host factory through the same multi-View module DSL. */
    public fun View(factory: WhiskerElementFactory): WhiskerDefinitionComponent =
        WhiskerViewComponent(
            components = emptyList(),
            elementName = factory.name,
            factory = factory,
        ).also { components.add(it) }

    /** Registers a Host factory with the ordinary View member block. */
    public fun View(
        factory: WhiskerElementFactory,
        block: WhiskerViewDefinitionBuilder.() -> Unit,
    ): WhiskerDefinitionComponent {
        val b = WhiskerViewDefinitionBuilder()
        b.block()
        return WhiskerViewComponent(
            components = b.components.toList(),
            elementName = factory.name,
            factory = factory,
        ).also { components.add(it) }
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

    /**
     * `AsyncFunction("getOfferings") { args, promise -> ...; promise.resolve(x) }`
     * — module-level async function. The author resolves/rejects the
     * [promise], now or from a completion callback.
     */
    public fun AsyncFunction(
        name: String,
        handler: (args: List<WhiskerValue>, promise: WhiskerPromise) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerAsyncFunctionComponent(name) { _, args, promise -> handler(args, promise) }
            .also { components.add(it) }
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
     * — view-bearing prop setter. `value` is the raw
     * [WhiskerValue]; the author destructures it. `V` is inferred
     * from the lambda; the dispatch-time view cast is unchecked
     * (erased generics) — a mismatch raises `ClassCastException`.
     */
    public fun <V : Any> Prop(
        name: String,
        clear: (V) -> Unit = {},
        setter: (V, WhiskerValue) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerPropComponent(
            name = name,
            setter = { viewAny, value ->
                @Suppress("UNCHECKED_CAST")
                setter(viewAny as V, value)
            },
            clearer = { viewAny ->
                @Suppress("UNCHECKED_CAST")
                clear(viewAny as V)
            },
        ).also { components.add(it) }

    /** `Events("a", "b", ...)` declared inside a `View(...)` block. */
    public fun Events(vararg names: String): WhiskerDefinitionComponent =
        WhiskerEventsComponent(names.toList()).also { components.add(it) }

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

    /** View-bound `AsyncFunction` — view + raw args + a [WhiskerPromise]. */
    public fun <V : Any> AsyncFunction(
        name: String,
        handler: (view: V, args: List<WhiskerValue>, promise: WhiskerPromise) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerAsyncFunctionComponent(name) { viewAny, args, promise ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, args, promise)
        }.also { components.add(it) }
}

// ----- ModuleDefinition value -----------------------------------------------

/**
 * The assembled definition the framework registers with the active Host at
 * module-init time. Immutable; collected from a
 * [WhiskerModuleDefinitionBuilder] block.
 */
public data class ModuleDefinition(public val components: List<WhiskerDefinitionComponent>) {

    /** Explicit module name, or the first directly declared element name. */
    public val name: String?
        get() = view?.elementName
            ?: components.firstNotNullOfOrNull { (it as? WhiskerNameComponent)?.value }

    /** First View block, retained for source compatibility. */
    public val view: WhiskerViewComponent?
        get() = views.firstOrNull()

    /** Every Host View contributed by this module. */
    internal val views: List<WhiskerViewComponent>
        get() = components.filterIsInstance<WhiskerViewComponent>()

    /** Merged constants from all [WhiskerConstantsComponent] blocks. */
    public val constants: Map<String, WhiskerValue>
        get() = buildMap {
            for (c in components) {
                if (c is WhiskerConstantsComponent) putAll(c.values)
            }
        }

    /** Module-level (view-less) [Function] declarations. */
    public val functions: List<WhiskerFunctionComponent>
        get() = components.filterIsInstance<WhiskerFunctionComponent>()

    /** Module-level (view-less) [AsyncFunction] declarations. */
    public val asyncFunctions: List<WhiskerAsyncFunctionComponent>
        get() = components.filterIsInstance<WhiskerAsyncFunctionComponent>()

    /** Module-level [OnStartObserving] hooks. */
    public val onStartObservingHooks: List<WhiskerOnStartObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStartObservingComponent>()

    /** Module-level [OnStopObserving] hooks. */
    public val onStopObservingHooks: List<WhiskerOnStopObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStopObservingComponent>()

    /** Validate declaration-local invariants before runtime negotiation. */
    public fun validateElementDeclaration() {
        for (view in views) {
            val props = view.components.filterIsInstance<WhiskerPropComponent>().map { it.name }
            require(props.toSet().size == props.size) {
                "duplicate Host property on ${view.elementName}"
            }
            val events = view.components.filterIsInstance<WhiskerEventsComponent>().flatMap { it.names }
            require(events.toSet().size == events.size) {
                "duplicate Host event on ${view.elementName}"
            }
        }
    }

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
