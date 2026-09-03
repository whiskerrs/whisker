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
//         View("whisker-video:Video", WhiskerVideoComponent::class.java) {
//             Prop("src") { view: WhiskerVideoComponent, value ->
//                 view.setSrc(value.asString() ?: "")
//             }
//             Command("play")  { view: WhiskerVideoComponent, _: WhiskerValue -> view.play()  }
//             Command("pause") { view: WhiskerVideoComponent, _: WhiskerValue -> view.pause() }
//             Command("seek")  { view: WhiskerVideoComponent, value: WhiskerValue ->
//                 value.asDouble()?.let(view::seek)
//             }
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
// `definition()` into generated Host registrations.

package rs.whisker.runtime

// ----- Component model -----------------------------------------------------

/**
 * Type-erased component the DSL collects. Concrete subtypes live
 * below. Authors normally don't reference [WhiskerDefinitionComponent]
 * directly — the factory functions (`Name`, `Prop`, `Function`,
 * etc.) return the right subtype.
 */
public sealed interface WhiskerDefinitionComponent

/** Components that are valid inside a `View(...)` declaration. */
public sealed interface WhiskerViewDefinitionComponent : WhiskerDefinitionComponent

/** `Name("Foo")` — the module's local tag name. */
public data class WhiskerNameComponent(public val value: String) :
    WhiskerDefinitionComponent

/**
 * `View("<crate>:Element", Foo::class.java) { ... }` — registers a native View class
 * + its explicit Rust element identity and inner DSL block (Prop / Command / Events). The class is
 * type-erased to `Class<*>` so the parent struct isn't generic;
 * the concrete class is typically a [WhiskerUI] subclass.
 */
public data class WhiskerViewComponent(
    public val viewClass: Class<*>? = null,
    public val components: List<WhiskerViewDefinitionComponent>,
    public val elementName: String? = null,
    internal val factory: WhiskerElementFactory? = null,
) : WhiskerDefinitionComponent

/**
 * Type-erased prop setter the framework calls on prop dispatch.
 * `view` is the native element instance; `value` is the raw
 * [WhiskerValue] — no auto-deserialization, the author destructures
 * it, e.g. `value.asString()`.
 */
public typealias WhiskerPropSetterFn = (view: Any, value: WhiskerValue) -> Unit
public typealias WhiskerPropClearerFn = (view: Any) -> Unit

public data class WhiskerPropComponent(
    public val name: String,
    public val setter: WhiskerPropSetterFn,
    public val clearer: WhiskerPropClearerFn,
) : WhiskerViewDefinitionComponent

/**
 * Module function handler. `args` are the raw positional [WhiskerValue]s from the Rust call
 * site — no auto-deserialization, the author destructures, e.g.
 * `args[0].asDouble()`; the return is a raw [WhiskerValue]
 * (`WhiskerValue.Null` for "no result").
 */
public typealias WhiskerFunctionHandlerFn = (args: List<WhiskerValue>) -> WhiskerValue

public data class WhiskerFunctionComponent(
    public val name: String,
    public val handler: WhiskerFunctionHandlerFn,
) : WhiskerDefinitionComponent

/**
 * Module ASYNC function handler. Like [WhiskerFunctionHandlerFn]
 * but instead of returning a value it resolves the given [promise] —
 * now or later, e.g. from a purchase / network completion. Mirrors Expo's
 * `AsyncFunction` + `Promise` (callback-resolved form).
 */
public typealias WhiskerAsyncFunctionHandlerFn =
    (args: List<WhiskerValue>, promise: WhiskerPromise) -> Unit

public data class WhiskerAsyncFunctionComponent(
    public val name: String,
    public val handler: WhiskerAsyncFunctionHandlerFn,
) : WhiskerDefinitionComponent

/** One-way View command. Element commands do not synchronously return data. */
public typealias WhiskerCommandHandlerFn = (view: Any, parameters: WhiskerValue) -> Unit

public data class WhiskerCommandComponent(
    public val name: String,
    public val handler: WhiskerCommandHandlerFn,
) : WhiskerViewDefinitionComponent

/** Resolved inherited text-style consumer for a native View. */
public typealias WhiskerTextStyleHandlerFn = (view: Any, style: WhiskerTextStyle) -> Unit

public data class WhiskerTextStyleComponent(
    public val handler: WhiskerTextStyleHandlerFn,
) : WhiskerViewDefinitionComponent

/** Synchronous intrinsic measurement provider for one View declaration. */
public data class WhiskerMeasurementComponent(
    public val handler: (WhiskerMeasureRequest) -> WhiskerMeasuredSize?,
) : WhiskerViewDefinitionComponent

/**
 * `Events("a", "b", ...)` — declare event names this module emits.
 * Metadata only; dispatch goes through
 * [Module.sendEvent], which fans the payload out to every Rust
 * subscriber registered via `PlatformModule::on_event`.
 */
public data class WhiskerEventsComponent(
    public val names: List<String>,
) : WhiskerViewDefinitionComponent

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
 * [Function] and [Events]) are **member functions** so
 * authors call them inside the `ModuleDefinition { ... }` block
 * without any `import` — and so `View(...)` doesn't collide with
 * `android.view.View` (a member on the implicit receiver wins over
 * an imported top-level / constructor name).
 *
 * They're plain (non-`inline`/non-`reified`) members; the
 * generic [WhiskerViewDefinitionBuilder.Prop] / `Command`
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
        WhiskerFunctionComponent(name, handler).also { components.add(it) }

    /**
     * `AsyncFunction("getOfferings") { args, promise -> ...; promise.resolve(x) }`
     * — module-level async function. The author resolves/rejects the
     * [promise], now or from a completion callback.
     */
    public fun AsyncFunction(
        name: String,
        handler: (args: List<WhiskerValue>, promise: WhiskerPromise) -> Unit,
    ): WhiskerDefinitionComponent =
        WhiskerAsyncFunctionComponent(name, handler).also { components.add(it) }
}

/**
 * Inner builder for the `View(...) { ... }` block. Same
 * member-function rationale as [WhiskerModuleDefinitionBuilder].
 * `@DslMarker` keeps the top-level factories ([Name], [View], …)
 * out of scope here so they can't be called inside a View block.
 */
@WhiskerDefinitionDsl
public class WhiskerViewDefinitionBuilder {
    internal val components: MutableList<WhiskerViewDefinitionComponent> = mutableListOf()

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
    ): WhiskerViewDefinitionComponent =
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
    public fun Events(vararg names: String): WhiskerViewDefinitionComponent =
        WhiskerEventsComponent(names.toList()).also { components.add(it) }

    /** Declares one ordered, one-way command on a mounted View. */
    public fun <V : Any> Command(
        name: String,
        handler: (view: V, parameters: WhiskerValue) -> Unit,
    ): WhiskerViewDefinitionComponent =
        WhiskerCommandComponent(name) { viewAny, parameters ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, parameters)
        }.also { components.add(it) }

    /** Receives the resolved inherited text style for this View. */
    public fun <V : Any> TextStyle(
        handler: (view: V, style: WhiskerTextStyle) -> Unit,
    ): WhiskerViewDefinitionComponent =
        WhiskerTextStyleComponent { viewAny, style ->
            @Suppress("UNCHECKED_CAST")
            handler(viewAny as V, style)
        }.also { components.add(it) }

    /** Supplies Host intrinsic metrics for Custom/ReplacedContent schemas. */
    public fun Measurement(
        handler: (WhiskerMeasureRequest) -> WhiskerMeasuredSize?,
    ): WhiskerViewDefinitionComponent =
        WhiskerMeasurementComponent(handler).also { components.add(it) }

}

// ----- ModuleDefinition value -----------------------------------------------

/**
 * The assembled definition the framework registers with the active Host at
 * module-init time. Immutable; collected from a
 * [WhiskerModuleDefinitionBuilder] block.
 */
public data class ModuleDefinition(public val components: List<WhiskerDefinitionComponent>) {

    /** Explicit module name. View identity is never a module-name fallback. */
    public val name: String?
        get() = components.firstNotNullOfOrNull { (it as? WhiskerNameComponent)?.value }

    /** First View block, retained for source compatibility. */
    public val view: WhiskerViewComponent?
        get() = views.firstOrNull()

    /** Every Host View contributed by this module. */
    internal val views: List<WhiskerViewComponent>
        get() = components.filterIsInstance<WhiskerViewComponent>()

    /** Module-level (view-less) [Function] declarations. */
    public val functions: List<WhiskerFunctionComponent>
        get() = components.filterIsInstance<WhiskerFunctionComponent>()

    /** Module-level (view-less) [AsyncFunction] declarations. */
    public val asyncFunctions: List<WhiskerAsyncFunctionComponent>
        get() = components.filterIsInstance<WhiskerAsyncFunctionComponent>()

    /** Module-scoped event declarations. View events remain inside their View block. */
    public val events: List<String>
        get() = components.filterIsInstance<WhiskerEventsComponent>().flatMap { it.names }

    /** Module-level [OnStartObserving] hooks. */
    public val onStartObservingHooks: List<WhiskerOnStartObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStartObservingComponent>()

    /** Module-level [OnStopObserving] hooks. */
    public val onStopObservingHooks: List<WhiskerOnStopObservingComponent>
        get() = components.filterIsInstance<WhiskerOnStopObservingComponent>()

    /** Validate declaration-local invariants before runtime negotiation. */
    public fun validateElementDeclaration() {
        val names = components.filterIsInstance<WhiskerNameComponent>()
        require(names.size == 1 && names.single().value.isNotBlank()) {
            "ModuleDefinition requires exactly one non-empty Name"
        }
        val functionNames = functions.map { it.name } + asyncFunctions.map { it.name }
        require(functionNames.all { it.isNotBlank() } && functionNames.toSet().size == functionNames.size) {
            "module Function and AsyncFunction names must be non-empty and unique"
        }
        require(events.all { it.isNotBlank() } && events.toSet().size == events.size) {
            "module Event names must be non-empty and unique"
        }
        val declaredEvents = events.toSet()
        val startEvents = onStartObservingHooks.map { it.eventName }
        val stopEvents = onStopObservingHooks.map { it.eventName }
        require(startEvents.toSet().size == startEvents.size) { "duplicate OnStartObserving hook" }
        require(stopEvents.toSet().size == stopEvents.size) { "duplicate OnStopObserving hook" }
        require((startEvents + stopEvents).all { it in declaredEvents }) {
            "observer hooks must reference a module Event declaration"
        }
        for (view in views) {
            require(!view.elementName.isNullOrBlank()) {
                "every View requires an explicit package-qualified element name"
            }
            val props = view.components.filterIsInstance<WhiskerPropComponent>().map { it.name }
            require(props.toSet().size == props.size) {
                "duplicate Host property on ${view.elementName}"
            }
            val events = view.components.filterIsInstance<WhiskerEventsComponent>().flatMap { it.names }
            require(events.size <= Long.SIZE_BITS && events.all { it.isNotBlank() } && events.toSet().size == events.size) {
                "duplicate Host event on ${view.elementName}"
            }
            val commands = view.components.filterIsInstance<WhiskerCommandComponent>().map { it.name }
            require(commands.toSet().size == commands.size) {
                "duplicate Host command on ${view.elementName}"
            }
            require(view.components.count { it is WhiskerTextStyleComponent } <= 1) {
                "duplicate TextStyle consumer on ${view.elementName}"
            }
            require(view.components.count { it is WhiskerMeasurementComponent } <= 1) {
                "duplicate Measurement provider on ${view.elementName}"
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
