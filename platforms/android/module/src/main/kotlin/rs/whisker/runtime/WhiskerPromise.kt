package rs.whisker.runtime

import java.util.concurrent.atomic.AtomicBoolean

/**
 * The resolver handed to an `AsyncFunction` handler (Android).
 *
 * Wraps the completion path of an async dispatch — the C bridge's
 * callback + opaque `user_data` (carried across JNI as raw `Long`
 * pointers) for module-level calls, or the Lynx UI-method callback for
 * view-bound calls — so a module can deliver its result *later*, e.g.
 * from a RevenueCat / coroutine completion, instead of returning
 * synchronously.
 *
 * Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
 * `resolve`/`reject` wins and fires the callback exactly once; further
 * calls are ignored, matching the Rust side whose `async_trampoline`
 * consumes the boxed sender on the first callback.
 */
public class WhiskerPromise internal constructor(
    private val onSettle: (WhiskerValue) -> Unit,
) {
    /** Module-dispatch form — constructed by
     *  [WhiskerModuleRegistry.invokeDispatchAsync]. */
    internal constructor(callbackPtr: Long, userDataPtr: Long) : this({ value ->
        if (callbackPtr != 0L) {
            WhiskerModuleRegistry.nativeResolveAsync(callbackPtr, userDataPtr, value)
        }
    })

    private val settled = AtomicBoolean(false)

    /** Resolve the call with [value] (`WhiskerValue.Null` for "no result"). */
    public fun resolve(value: WhiskerValue) {
        fire(value)
    }

    /** Reject with an error message — surfaces as `WhiskerValue.Err` on the
     *  Rust side (the awaiting `invoke_async` future resolves to it). */
    public fun reject(message: String) {
        fire(WhiskerValue.Err(message))
    }

    private fun fire(value: WhiskerValue) {
        if (!settled.compareAndSet(false, true)) return
        onSettle(value)
    }
}
