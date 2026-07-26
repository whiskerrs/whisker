// The resolver handed to an `AsyncFunction` handler (iOS).
//
// Wraps the C bridge's completion `callback` + opaque `user_data` (the
// Rust `oneshot::Sender` boxed by `PlatformModule::invoke_async`), so a
// module can deliver its result *later* — e.g. from a StoreKit purchase
// or `URLSession` completion — instead of returning synchronously.
//
// Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
// `resolve`/`reject` wins and fires the callback exactly once; further
// calls are ignored. This matches the Rust side, whose `async_trampoline`
// consumes the boxed sender on the first callback and would have nothing
// left to resolve on a second.

import Foundation

public final class WhiskerPromise {
    private let callback: WhiskerModuleCallback
    private let userData: UnsafeMutableRawPointer?
    private let lock = NSLock()
    private var settled = false

    /// `callback` is the bridge's `WhiskerModuleCallback`; `userData` the
    /// opaque pointer the async dispatch was handed. Constructed only by
    /// the module-dispatch layer.
    init(callback: WhiskerModuleCallback, userData: UnsafeMutableRawPointer?) {
        self.callback = callback
        self.userData = userData
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

        // Same encode → hand-to-bridge → release sequence the event
        // center uses (`WhiskerModuleEventCenter.dispatchSend`): the
        // callback copies the value out before returning, so releasing
        // right after is safe.
        var raw = value.toRaw()
        callback(userData, &raw)
        whisker_bridge_value_release(&raw)
    }
}
