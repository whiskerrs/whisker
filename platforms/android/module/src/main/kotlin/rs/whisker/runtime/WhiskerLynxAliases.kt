package rs.whisker.runtime

/**
 * Lynx symbol hiding (Android).
 *
 * A view-bearing Whisker module's `View(...)` block references a
 * Lynx UI subclass. These typealiases give module authors
 * `Whisker*` symbols that resolve to their Lynx counterparts at
 * Kotlin's type-system level — same runtime classes, just a
 * presentation rename, so Lynx-ness doesn't leak into every
 * module's public API:
 *
 * ```kotlin
 * import rs.whisker.runtime.WhiskerContext
 * import rs.whisker.runtime.WhiskerUI
 *
 * class HelloView(context: WhiskerContext) : WhiskerUI<View>(context) { ... }
 * ```
 *
 * Stack traces / debugger views still surface the real `LynxUI`
 * class names — typealiases are purely a source-level concept.
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
 * the host's bridge reporter, which delivers `params` to the
 * matching Rust `on_<event>` callback.
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
