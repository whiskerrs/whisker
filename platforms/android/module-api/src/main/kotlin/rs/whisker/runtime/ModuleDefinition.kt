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

@WhiskerDefinitionDsl
public class WhiskerModuleDefinitionBuilder {
    /**
     * Internal mutable collection of components the factory
     * functions ([Name], [View], [Function], [Constants], [Events])
     * append to as they execute. The lambda block's receiver is a
     * [WhiskerModuleDefinitionBuilder], so calls inside the block
     * resolve to the extension functions defined below — each one
     * has the side effect of appending its component here.
     */
    @PublishedApi
    internal val components: MutableList<WhiskerDefinitionComponent> = mutableListOf()
}

@WhiskerDefinitionDsl
public class WhiskerViewDefinitionBuilder {
    @PublishedApi
    internal val components: MutableList<WhiskerDefinitionComponent> = mutableListOf()
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

// ----- Top-level factories — the DSL surface --------------------------------

// Naming convention: PascalCase, mirroring Expo Modules + the
// Swift side. Kotlin allows top-level fn names of any case.

/** `Name("Foo")` — the module's local tag name. */
public fun WhiskerModuleDefinitionBuilder.Name(value: String): WhiskerDefinitionComponent {
    val c = WhiskerNameComponent(value)
    components.add(c)
    return c
}

/** `Constants("k" to v, ...)` — static dictionary. */
public fun WhiskerModuleDefinitionBuilder.Constants(
    vararg entries: Pair<String, Any?>,
): WhiskerDefinitionComponent {
    val c = WhiskerConstantsComponent(entries.toMap())
    components.add(c)
    return c
}

/** `Constants(mapOf(...))` — same, but takes a Map directly. */
public fun WhiskerModuleDefinitionBuilder.Constants(
    values: Map<String, Any?>,
): WhiskerDefinitionComponent {
    val c = WhiskerConstantsComponent(values)
    components.add(c)
    return c
}

/**
 * `View(MyView::class.java) { ... }` — registers a Lynx UI
 * subclass + its inner DSL block (Prop / Function / Events).
 */
public fun WhiskerModuleDefinitionBuilder.View(
    viewClass: Class<*>,
    block: WhiskerViewDefinitionBuilder.() -> Unit,
): WhiskerDefinitionComponent {
    val b = WhiskerViewDefinitionBuilder()
    b.block()
    val c = WhiskerViewComponent(viewClass, b.components.toList())
    components.add(c)
    return c
}

/** `Events("a", "b", ...)` — variadic event-name declaration. */
public fun WhiskerModuleDefinitionBuilder.Events(
    vararg names: String,
): WhiskerDefinitionComponent {
    val c = WhiskerEventsComponent(names.toList())
    components.add(c)
    return c
}

/** Events declared inside a `View(...)` block. */
public fun WhiskerViewDefinitionBuilder.Events(
    vararg names: String,
): WhiskerDefinitionComponent {
    val c = WhiskerEventsComponent(names.toList())
    components.add(c)
    return c
}

// ----- Prop factories ------------------------------------------------------

/**
 * `Prop("foo") { view, value -> ... }` — view-bearing prop setter.
 * Inside a `View(...)` block.
 *
 * Reified type parameters let the framework downcast at dispatch
 * time. Mismatches silently no-op (with a debug-build log).
 */
public inline fun <reified V : Any, reified T> WhiskerViewDefinitionBuilder.Prop(
    name: String,
    noinline setter: (V, T) -> Unit,
): WhiskerDefinitionComponent {
    val c = WhiskerPropComponent(name) { viewAny: Any, valueAny: Any? ->
        val v = viewAny as? V
        if (v == null) {
            // Type mismatch — drop the call.
            return@WhiskerPropComponent
        }
        @Suppress("UNCHECKED_CAST")
        val t = valueAny as? T
        if (t == null && valueAny != null) {
            return@WhiskerPropComponent
        }
        @Suppress("UNCHECKED_CAST")
        setter(v, t as T)
    }
    components.add(c)
    return c
}

// ----- Function factories — overloads for 0..2 args -------------------------

// View-bound functions: first arg is the view type.

public inline fun <reified V : Any> WhiskerViewDefinitionBuilder.Function(
    name: String,
    crossinline handler: (V) -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { viewAny: Any?, _: List<Any?> ->
        val v = viewAny as? V ?: return@WhiskerFunctionComponent null
        handler(v)
    }
    components.add(c)
    return c
}

public inline fun <reified V : Any, reified A> WhiskerViewDefinitionBuilder.Function(
    name: String,
    crossinline handler: (V, A) -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { viewAny: Any?, args: List<Any?> ->
        val v = viewAny as? V ?: return@WhiskerFunctionComponent null
        val a = args.firstOrNull() as? A ?: return@WhiskerFunctionComponent null
        handler(v, a)
    }
    components.add(c)
    return c
}

public inline fun <reified V : Any, reified A, reified B> WhiskerViewDefinitionBuilder.Function(
    name: String,
    crossinline handler: (V, A, B) -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { viewAny: Any?, args: List<Any?> ->
        val v = viewAny as? V ?: return@WhiskerFunctionComponent null
        if (args.size < 2) return@WhiskerFunctionComponent null
        val a = args[0] as? A ?: return@WhiskerFunctionComponent null
        val b = args[1] as? B ?: return@WhiskerFunctionComponent null
        handler(v, a, b)
    }
    components.add(c)
    return c
}

// Module-level (view-less) functions: no view argument.

public fun WhiskerModuleDefinitionBuilder.Function(
    name: String,
    handler: () -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { _: Any?, _: List<Any?> -> handler() }
    components.add(c)
    return c
}

public inline fun <reified A> WhiskerModuleDefinitionBuilder.Function(
    name: String,
    crossinline handler: (A) -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { _: Any?, args: List<Any?> ->
        val a = args.firstOrNull() as? A ?: return@WhiskerFunctionComponent null
        handler(a)
    }
    components.add(c)
    return c
}

public inline fun <reified A, reified B> WhiskerModuleDefinitionBuilder.Function(
    name: String,
    crossinline handler: (A, B) -> Any?,
): WhiskerDefinitionComponent {
    val c = WhiskerFunctionComponent(name) { _: Any?, args: List<Any?> ->
        if (args.size < 2) return@WhiskerFunctionComponent null
        val a = args[0] as? A ?: return@WhiskerFunctionComponent null
        val b = args[1] as? B ?: return@WhiskerFunctionComponent null
        handler(a, b)
    }
    components.add(c)
    return c
}
