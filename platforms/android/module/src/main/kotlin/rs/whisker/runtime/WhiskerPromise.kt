package rs.whisker.runtime

import java.util.concurrent.atomic.AtomicBoolean

/**
 * The resolver handed to an `AsyncFunction` handler (Android).
 *
 * Wraps the Host-provided completion closure so a module can deliver its
 * result later, e.g. from a RevenueCat or coroutine completion.
 *
 * Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
 * `resolve`/`reject` wins and fires the callback exactly once; further
 * calls are ignored.
 */
public class WhiskerPromise internal constructor(
    private val onSettle: (WhiskerValue) -> Unit,
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
        if (!settled.compareAndSet(false, true)) return
        onSettle(value)
    }
}
