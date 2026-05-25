// Phase L-2c — Android dispatch wiring for the `ModuleDefinition` DSL.
//
// At module-registration time (host-app launch), the framework
// calls `module.registerWithLynx()`. This walks the DSL definition
// the author built in `definition()` and installs:
//
//   - One [com.lynx.tasm.behavior.utils.LynxUISetter] per `View(...)`
//     block that dispatches every `Prop("...") { setter }` declared
//     inside it.
//   - One [com.lynx.tasm.behavior.utils.LynxUIMethodInvoker] per
//     `View(...)` block that dispatches every `Function("...") { handler }`
//     declared inside it.
//
// Both are registered against the Lynx-fork-public Class-explicit
// overloads added in Phase L-1 (`v3.7.0-whisker.4`):
//
//   PropsUpdater.registerSetter(Class, Settable)
//   LynxUIMethodsExecutor.registerMethodInvoker(Class, Invoker)
//
// keyed by the target UI class declared in `View(MyView::class.java) {...}`.
//
// ## Coexistence with the annotation API
//
// Both paths register against the same Lynx prop / method dispatch
// tables. A class registered through this DSL path replaces (last-
// write-wins) any prior reflection-based registration for the same
// target class — which is what we want during the L-3 sample-
// migration window where some modules use the DSL and others stay
// on `@WhiskerComponent`.

package rs.whisker.runtime

import com.lynx.react.bridge.Callback
import com.lynx.react.bridge.ReadableMap
import com.lynx.tasm.behavior.StylesDiffMap
import com.lynx.tasm.behavior.ui.LynxBaseUI
import com.lynx.tasm.behavior.utils.LynxUIMethodInvoker
import com.lynx.tasm.behavior.utils.LynxUISetter
import com.lynx.tasm.behavior.LynxUIMethodConstants
import com.lynx.tasm.behavior.utils.LynxUIMethodsExecutor
import com.lynx.tasm.behavior.utils.PropsUpdater

/**
 * Walk [definitionLazy] and install the DSL-declared surface.
 *
 * Two shapes:
 *
 *  - **View-bearing** (`View(...) { Prop / Function }`): installs a
 *    [LynxUISetter] + [LynxUIMethodInvoker] on the view class via
 *    the L-1 Class-explicit registration APIs.
 *  - **View-less** (module-level `Function`s, no `View(...)`):
 *    registers a dispatch closure with [WhiskerModuleRegistry]
 *    under the module's `Name(...)`, so `whisker_bridge_invoke_module`
 *    from Rust routes into the DSL handlers — the same path the
 *    legacy `@WhiskerModule` annotation used.
 *
 * Idempotent — re-registering replaces the prior entry
 * (last-write-wins on both maps).
 */
public fun Module.registerWithLynx() {
    val def = definitionLazy
    val viewBlock = def.view

    if (viewBlock != null) {
        registerViewBearing(viewBlock)
    } else {
        registerViewLess(def)
    }
}

private fun registerViewBearing(viewBlock: WhiskerViewComponent) {
    @Suppress("UNCHECKED_CAST")
    val viewClass: Class<out LynxBaseUI> =
        viewBlock.viewClass as Class<out LynxBaseUI>

    val propComponents = viewBlock.components.filterIsInstance<WhiskerPropComponent>()
    val funcComponents = viewBlock.components.filterIsInstance<WhiskerFunctionComponent>()

    if (propComponents.isNotEmpty()) {
        PropsUpdater.registerSetter(
            viewClass,
            WhiskerDSLPropsSetter(propComponents),
        )
    }
    if (funcComponents.isNotEmpty()) {
        LynxUIMethodsExecutor.registerMethodInvoker(
            viewClass,
            WhiskerDSLMethodInvoker(funcComponents),
        )
    }
}

private fun registerViewLess(def: ModuleDefinition) {
    val name = def.name ?: return
    val functions = def.functions
    if (functions.isEmpty()) return

    val byName = functions.associateBy { it.name }
    WhiskerModuleRegistry.registerDispatch(name) { method, args ->
        val fn = byName[method]
            ?: return@registerDispatch WhiskerValue.Err("unknown method `$method` on module `$name`")
        // Unwrap the WhiskerValue args into raw Kotlin values the
        // handler's `as A` casts expect, run the closure, then wrap
        // the result back into a WhiskerValue.
        val rawArgs: List<Any?> = args.map { it.toJavaObject() }
        val result: Any? = try {
            fn.handler(null, rawArgs)
        } catch (t: Throwable) {
            return@registerDispatch WhiskerValue.Err(
                "exception in module `$name` method `$method`: ${t.message}",
            )
        }
        whiskerValueOf(result)
    }
}

// ----- LynxUISetter adapter ------------------------------------------------

/**
 * Generic [LynxUISetter] that dispatches every prop declared inside
 * a `View(...)` block by looking up the matching
 * [WhiskerPropComponent] and calling its closure.
 *
 * Lynx calls `setProperty(ui, propName, props)` once per changed
 * prop; we look up by name and route into the DSL closure with the
 * decoded value.
 */
internal class WhiskerDSLPropsSetter(
    private val props: List<WhiskerPropComponent>,
) : LynxUISetter<LynxBaseUI> {

    // Lookup by name. Index-once on construction; the underlying
    // list is immutable.
    private val byName: Map<String, WhiskerPropComponent> =
        props.associateBy { it.name }

    override fun setProperty(ui: LynxBaseUI, name: String, propsMap: StylesDiffMap) {
        val component = byName[name] ?: return
        // Decode the Lynx prop value to a Kotlin Any?. The Dynamic
        // surface here mirrors what `PropsSetterCache.PropSetter`
        // does internally: `getDynamic(name)` returns a tagged
        // value, and we unbox via the type-specific accessor on
        // first dispatch.
        //
        // L-2c keeps the unboxing minimal: we hand the raw boxed
        // Java object to the DSL closure, which uses Kotlin's
        // `as? T` downcast to validate. Type-aware decoding lives
        // in a follow-up that propagates the Prop's declared
        // value type through `WhiskerPropComponent`.
        val value: Any? = decodeProp(propsMap, name)
        component.setter(ui, value)
    }

    private fun decodeProp(propsMap: StylesDiffMap, name: String): Any? {
        val dyn = propsMap.getDynamic(name) ?: return null
        // The Dynamic API surfaces values via type-specific
        // getters. We try the common cases in order — what Lynx
        // hands us is determined by the JS / Rust side's encoded
        // value, not the Kotlin closure's declared type, so
        // probing the type tag is the only safe path.
        return try {
            when (dyn.type) {
                com.lynx.react.bridge.ReadableType.Boolean -> dyn.asBoolean()
                com.lynx.react.bridge.ReadableType.Int -> dyn.asInt()
                com.lynx.react.bridge.ReadableType.Number -> dyn.asDouble()
                com.lynx.react.bridge.ReadableType.String -> dyn.asString()
                com.lynx.react.bridge.ReadableType.Map -> dyn.asMap()
                com.lynx.react.bridge.ReadableType.Array -> dyn.asArray()
                else -> null
            }
        } catch (_: Throwable) {
            null
        }
    }
}

// ----- LynxUIMethodInvoker adapter -----------------------------------------

/**
 * Generic [LynxUIMethodInvoker] that dispatches every function
 * declared inside a `View(...)` block by looking up the matching
 * [WhiskerFunctionComponent] and calling its closure.
 *
 * Lynx calls `invoke(ui, methodName, params, callback)` once per
 * `lynxUI.invoke(...)` from the JS / Rust side. We look up by
 * name, decode the args array Lynx packs into `params`, run the
 * closure, and resolve the callback with the result (or a
 * NODE_NOT_FOUND error if the method isn't declared).
 *
 * Params convention: Lynx's reflection-based path packs positional
 * args as `{"args": [v1, v2, ...]}` (matching what the existing
 * `@WhiskerUIMethod` macro decodes). We preserve that contract so
 * the same Rust-side `ElementRef::invoke(...)` call site can route
 * to either the annotation path or the DSL path interchangeably.
 */
internal class WhiskerDSLMethodInvoker(
    private val functions: List<WhiskerFunctionComponent>,
) : LynxUIMethodInvoker<LynxBaseUI> {

    private val byName: Map<String, WhiskerFunctionComponent> =
        functions.associateBy { it.name }

    override fun invoke(
        ui: LynxBaseUI,
        methodName: String,
        params: ReadableMap?,
        callback: Callback,
    ) {
        val component = byName[methodName]
        if (component == null) {
            callback.invoke(
                LynxUIMethodConstants.METHOD_NOT_FOUND,
                "unknown method `$methodName`",
            )
            return
        }
        val rawArgs: List<Any?> = decodeArgs(params)
        val result: Any? = try {
            component.handler(ui, rawArgs)
        } catch (t: Throwable) {
            callback.invoke(
                LynxUIMethodConstants.UNKNOWN,
                "exception while invoking `$methodName`: ${t.message}",
            )
            return
        }
        callback.invoke(LynxUIMethodConstants.SUCCESS, result)
    }

    private fun decodeArgs(params: ReadableMap?): List<Any?> {
        if (params == null) return emptyList()
        return try {
            val arr = params.getArray("args") ?: return emptyList()
            buildList(capacity = arr.size()) {
                for (i in 0 until arr.size()) {
                    add(
                        when (arr.getType(i)) {
                            com.lynx.react.bridge.ReadableType.Boolean -> arr.getBoolean(i)
                            com.lynx.react.bridge.ReadableType.Int -> arr.getInt(i)
                            com.lynx.react.bridge.ReadableType.Number -> arr.getDouble(i)
                            com.lynx.react.bridge.ReadableType.String -> arr.getString(i)
                            com.lynx.react.bridge.ReadableType.Map -> arr.getMap(i)
                            com.lynx.react.bridge.ReadableType.Array -> arr.getArray(i)
                            else -> null
                        }
                    )
                }
            }
        } catch (_: Throwable) {
            emptyList()
        }
    }
}
