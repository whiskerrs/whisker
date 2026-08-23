import UIKit

/**
 Process-wide safe-area source shared by `WhiskerView` and native modules.

 A module subscribes without importing the concrete application-owned
 `WhiskerView` type. Multiple views are supported: the most recently
 attached live view is the active source, matching `WhiskerAppContext` on
 Android.
 */
public enum WhiskerInsetsDispatcher {
    public final class Registration {
        fileprivate let id = UUID()
        fileprivate let callback: (UIEdgeInsets) -> Void

        fileprivate init(_ callback: @escaping (UIEdgeInsets) -> Void) {
            self.callback = callback
        }
    }

    private final class WeakView {
        weak var value: UIView?
        init(_ value: UIView) { self.value = value }
    }

    private static let lock = NSLock()
    private static var views: [WeakView] = []
    private static var listeners: [UUID: Registration] = [:]

    /** Register a view as the current inset source. Called by `WhiskerView`. */
    public static func attach(_ view: UIView) {
        let callbacks: [Registration]
        lock.lock()
        views.removeAll { $0.value == nil || $0.value === view }
        views.append(WeakView(view))
        callbacks = Array(listeners.values)
        lock.unlock()
        notify(callbacks, insets: view.safeAreaInsets)
    }

    /** Remove a view from the source stack. Called when `WhiskerView` detaches. */
    public static func detach(_ view: UIView) {
        let current: UIView?
        let callbacks: [Registration]
        lock.lock()
        views.removeAll { $0.value == nil || $0.value === view }
        current = views.last?.value
        callbacks = Array(listeners.values)
        lock.unlock()
        if let current { notify(callbacks, insets: current.safeAreaInsets) }
    }

    /** Publish a changed inset value from an attached view. */
    public static func update(_ view: UIView) {
        let callbacks: [Registration]
        lock.lock()
        views.removeAll { $0.value == nil }
        guard views.last?.value === view else {
            lock.unlock()
            return
        }
        callbacks = Array(listeners.values)
        lock.unlock()
        notify(callbacks, insets: view.safeAreaInsets)
    }

    /** Subscribe and immediately receive the current value when available. */
    @discardableResult
    public static func addListener(
        _ callback: @escaping (UIEdgeInsets) -> Void
    ) -> Registration {
        let registration = Registration(callback)
        let current: UIView?
        lock.lock()
        views.removeAll { $0.value == nil }
        listeners[registration.id] = registration
        current = views.last?.value
        lock.unlock()
        if let current { callback(current.safeAreaInsets) }
        return registration
    }

    public static func removeListener(_ registration: Registration) {
        lock.lock()
        listeners.removeValue(forKey: registration.id)
        lock.unlock()
    }

    private static func notify(_ registrations: [Registration], insets: UIEdgeInsets) {
        for registration in registrations { registration.callback(insets) }
    }
}
