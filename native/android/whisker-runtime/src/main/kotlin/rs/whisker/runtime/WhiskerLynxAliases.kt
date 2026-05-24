package rs.whisker.runtime

/**
 * Phase 7-Φ.H.1: Lynx symbol hiding (Android).
 *
 * Module authors writing `@WhiskerElement(...)`-annotated classes
 * previously had to import Lynx types directly:
 *
 * ```kotlin
 * import com.lynx.tasm.behavior.LynxContext
 * import com.lynx.tasm.behavior.ui.LynxUI
 *
 * @WhiskerElement("Hello")
 * class WhiskerHelloElement(context: LynxContext) : LynxUI<View>(context) { ... }
 * ```
 *
 * The bridge is built on Lynx and that won't change in the
 * foreseeable future, but the Lynx-ness leaking into every
 * module's public API surface makes Whisker feel like a thin
 * Lynx wrapper. These typealiases give module authors
 * `Whisker*` symbols that resolve to their Lynx counterparts at
 * Kotlin's type-system level — same runtime classes, just a
 * presentation rename.
 *
 * ```kotlin
 * import rs.whisker.runtime.WhiskerContext
 * import rs.whisker.runtime.WhiskerUI
 *
 * @WhiskerElement("Hello")
 * class WhiskerHelloElement(context: WhiskerContext) : WhiskerUI<View>(context) { ... }
 * ```
 *
 * Stack traces / debugger views still surface the real `LynxUI`
 * class names (typealiases are purely a source-level concept).
 * Renaming the underlying classes themselves would require
 * patching the Lynx fork — a separate, larger effort planned
 * for the long-term roadmap.
 *
 * Note: Kotlin's `typealias` keyword cannot alias annotation
 * types. `@LynxProp` therefore needs a separate KSP-forwarder
 * mechanism (see `@WhiskerProp` + the KSP processor) rather than
 * a typealias here.
 */

public typealias WhiskerUI<V> = com.lynx.tasm.behavior.ui.LynxUI<V>

public typealias WhiskerContext = com.lynx.tasm.behavior.LynxContext

public typealias WhiskerCustomEventBase = com.lynx.tasm.event.LynxCustomEvent

public typealias WhiskerBehavior = com.lynx.tasm.behavior.Behavior

public typealias WhiskerEnv = com.lynx.tasm.LynxEnv

// MARK: - Custom-event dispatch helper

/**
 * Whisker-branded façade over `LynxCustomEvent` +
 * `LynxContext.eventEmitter.dispatchCustomEvent(...)`.
 *
 * Module authors that need to push events back to Rust (e.g. an
 * `Input` element's text-change firing `on_input:` on the consumer
 * crate) call:
 *
 * ```kotlin
 * WhiskerCustomEvent.dispatch(
 *     from = this,                                    // WhiskerUI subclass
 *     name = "input",
 *     params = mapOf("value" to editText.text.toString()))
 * ```
 *
 * instead of manually constructing `LynxCustomEvent` and
 * reaching into `lynxContext.eventEmitter`. The function looks
 * at the UI's `sign` + `lynxContext` to wire the event back to
 * the host's bridge reporter, which delivers the JSON-serialised
 * params to the matching Rust `on_<event>: String` callback.
 */
public object WhiskerCustomEvent {
    /**
     * Build and dispatch a `LynxCustomEvent` from [ui]. No-op if
     * the UI's context is null (e.g. before mount or after
     * detach).
     */
    @JvmStatic
    public fun dispatch(
        ui: WhiskerUI<*>,
        name: String,
        params: Map<String, Any?> = emptyMap(),
    ) {
        val ctx = ui.lynxContext ?: return
        val emitter = ctx.eventEmitter ?: return
        val event = com.lynx.tasm.event.LynxCustomEvent(ui.sign, name, params)
        // Android's `EventEmitter` exposes `sendCustomEvent(...)`
        // (whereas iOS's equivalent is `dispatchCustomEvent`).
        // Same end behaviour — the reporter sees the event.
        emitter.sendCustomEvent(event)
    }
}
