package rs.whisker.runtime

import java.util.concurrent.atomic.AtomicBoolean

/**
 * The resolver handed to an `AsyncFunction` handler (Android).
 *
 * Wraps the C bridge's completion callback + opaque `user_data` (the
 * Rust `oneshot::Sender` boxed by `PlatformModule::invoke_async`),
 * carried across JNI as raw `Long` pointers, so a module can deliver
 * its result *later* — e.g. from a RevenueCat / coroutine completion —
 * instead of returning synchronously.
 *
 * Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
 * `resolve`/`reject` wins and fires the callback exactly once; further
 * calls are ignored, matching the Rust side whose `async_trampoline`
 * consumes the boxed sender on the first callback.
 *
 * Constructed only by [WhiskerModuleRegistry.invokeDispatchAsync].
 */
public class WhiskerPromise internal constructor(
    private val callbackPtr: Long,
    private val userDataPtr: Long,
) {
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
        if (callbackPtr == 0L) return
        if (!settled.compareAndSet(false, true)) return
        WhiskerModuleRegistry.nativeResolveAsync(callbackPtr, userDataPtr, value)
    }
}
