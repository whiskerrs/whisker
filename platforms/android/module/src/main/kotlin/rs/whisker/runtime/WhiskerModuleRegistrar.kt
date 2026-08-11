// Android dispatch wiring for the `ModuleDefinition` DSL.
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
// Both are registered against the Class-explicit overloads the Lynx
// fork makes public:
//
//   PropsUpdater.registerSetter(Class, Settable)
//   LynxUIMethodsExecutor.registerMethodInvoker(Class, Invoker)
//
// keyed by the target UI class declared in `View(MyView::class.java) {...}`.
// Registration is last-write-wins per target class.

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
 *    the Class-explicit registration APIs.
 *  - **View-less** (module-level `Function`s, no `View(...)`):
 *    registers a dispatch closure with [WhiskerModuleRegistry]
 *    under the module's `Name(...)`, so `whisker_bridge_invoke_module`
 *    from Rust routes into the DSL handlers.
 *
 * Idempotent — re-registering replaces the prior entry
 * (last-write-wins on both maps).
 */
public fun Module.registerWithLynx(crateName: String? = null) {
    val def = definitionLazy
    val viewBlock = def.view

    if (viewBlock != null) {
        // View props / methods dispatch on the view CLASS, so no
        // name prefix is needed here.
        registerViewBearing(viewBlock)
    } else {
        // View-less dispatch is keyed by module name — namespace it
        // with the crate so two crates can ship same-named modules.
        registerViewLess(def, crateName)
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

private fun registerViewLess(def: ModuleDefinition, crateName: String?) {
    val name = def.name ?: return
    val functions = def.functions
    val asyncFunctions = def.asyncFunctions
    if (functions.isEmpty() && asyncFunctions.isEmpty()) return

    // `<crate>:<Name>` so the Rust side's `module!("Name")` (which
    // prepends its own crate name) resolves to the same key.
    val qualifiedName = if (crateName.isNullOrEmpty()) name else "$crateName:$name"

    if (functions.isNotEmpty()) {
        val byName = functions.associateBy { it.name }
        WhiskerModuleRegistry.registerDispatch(qualifiedName) { method, args ->
            val fn = byName[method]
                ?: return@registerDispatch WhiskerValue.Err("unknown method `$method` on module `$name`")
            try {
                fn.handler(null, args.asList())
            } catch (t: Throwable) {
                WhiskerValue.Err("exception in module `$name` method `$method`: ${t.message}")
            }
        }
    }

    if (asyncFunctions.isNotEmpty()) {
        val asyncByName = asyncFunctions.associateBy { it.name }
        WhiskerModuleRegistry.registerDispatchAsync(qualifiedName) { method, args, promise ->
            val fn = asyncByName[method] ?: return@registerDispatchAsync false
            // The handler owns the promise; it resolves now or later.
            // Exceptions surface via `invokeDispatchAsync`'s catch
            // (→ promise.reject).
            fn.handler(null, args.asList(), promise)
            true
        }
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

    private val byName: Map<String, WhiskerPropComponent> =
        props.associateBy { it.name }

    override fun setProperty(ui: LynxBaseUI, name: String, propsMap: StylesDiffMap) {
        val component = byName[name] ?: return
        component.setter(ui, whiskerValueOf(decodeProp(propsMap, name)))
    }

    private fun decodeProp(propsMap: StylesDiffMap, name: String): Any? {
        val dyn = propsMap.getDynamic(name) ?: return null
        // What Lynx hands us is determined by the JS / Rust side's
        // encoded value, not the Kotlin closure's declared type, so
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
 * Params convention: Lynx packs positional args as
 * `{"args": [v1, v2, ...]}`. Preserving that contract is what lets
 * the Rust-side `ElementRef::invoke(...)` call site stay uniform.
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
        val args: List<WhiskerValue> = decodeArgs(params).map { whiskerValueOf(it) }
        val result: WhiskerValue = try {
            component.handler(ui, args)
        } catch (t: Throwable) {
            callback.invoke(
                LynxUIMethodConstants.UNKNOWN,
                "exception while invoking `$methodName`: ${t.message}",
            )
            return
        }
        callback.invoke(LynxUIMethodConstants.SUCCESS, result.toJavaObject())
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
