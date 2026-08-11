// The resolver handed to an `AsyncFunction` handler (iOS).
//
// Wraps the completion path of an async dispatch — the C bridge's
// `callback` + opaque `user_data` for module-level calls, or the Lynx
// UI-method callback for view-bound calls — so a module can deliver
// its result *later* (e.g. from a StoreKit purchase or `URLSession`
// completion) instead of returning synchronously.
//
// Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
// `resolve`/`reject` wins and fires the callback exactly once; further
// calls are ignored. This matches the Rust side, whose `async_trampoline`
// consumes the boxed sender on the first callback and would have nothing
// left to resolve on a second.

import Foundation

public final class WhiskerPromise {
    private let onSettle: (WhiskerValue) -> Void
    private let lock = NSLock()
    private var settled = false

    /// Module-dispatch form: `callback` is the bridge's
    /// `WhiskerModuleCallback`; `userData` the opaque pointer the async
    /// dispatch was handed.
    init(callback: WhiskerModuleCallback, userData: UnsafeMutableRawPointer?) {
        // Same encode → hand-to-bridge → release sequence the event
        // center uses (`WhiskerModuleEventCenter.dispatchSend`): the
        // callback copies the value out before returning, so releasing
        // right after is safe.
        self.onSettle = { value in
            var raw = value.toRaw()
            callback(userData, &raw)
            whisker_bridge_value_release(&raw)
        }
    }

    /// Closure form — constructed by the Lynx UI-method dispatch layer
    /// (`WhiskerLynxInstaller.installAsyncFunction`).
    init(onSettle: @escaping (WhiskerValue) -> Void) {
        self.onSettle = onSettle
    }

    /// Resolve the call with `value` (`.null` for "no result").
    public func resolve(_ value: WhiskerValue) {
        fire(value)
    }

    /// Reject with an error message — surfaces as `WhiskerValue.error` on
    /// the Rust side (the awaiting `invoke_async` future resolves to it).
    public func reject(_ message: String) {
        fire(.error(message))
    }

    private func fire(_ value: WhiskerValue) {
        lock.lock()
        if settled {
            lock.unlock()
            return
        }
        settled = true
        lock.unlock()

        onSettle(value)
    }
}
