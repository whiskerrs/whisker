// The resolver handed to an `AsyncFunction` handler (iOS).
//
// Wraps the Host-provided completion closure so a module can deliver its
// result *later* (e.g. from a StoreKit purchase or `URLSession` completion)
// instead of returning synchronously.
//
// Mirrors Expo's `Promise` (`resolve` / `reject`). One-shot: the first
// `resolve`/`reject` wins and fires the callback exactly once; further
// calls are ignored.

import Foundation

public final class WhiskerPromise {
    private let onSettle: (WhiskerValue) -> Void
    private let lock = NSLock()
    private var settled = false

    /// Host runtimes provide the completion closure when dispatching an
    /// asynchronous module function.
    public init(onSettle: @escaping (WhiskerValue) -> Void) {
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
