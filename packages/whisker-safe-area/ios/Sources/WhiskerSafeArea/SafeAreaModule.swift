// `whisker-safe-area` Module (iOS).
//
// View-less module. Subscribes to `WhiskerView`'s
// `safeAreaInsetsDidChangeNotification` once at least one Rust
// listener is registered against the `insetsChanged` event; converts
// the firing view's `UIEdgeInsets` into a `WhiskerValue.map` payload
// and dispatches.
//
// The Rust side (`packages/whisker-safe-area/src/lib.rs`) is the only
// consumer — it holds the `RwSignal<SafeAreaInsets>` `safe_area_insets()`
// returns and updates it from this module's events.
//
// ## Lifecycle
//
// `OnStartObserving` must also push the current insets of any
// already-attached `WhiskerView`, or a component that mounts after the
// host view finished laying out leaves the signal stuck at `default()`.
// The bridge guarantees `OnStopObserving` fires on the 1→0 transition,
// so the observer closure can't leak.

import Foundation
import UIKit
import WhiskerModule
import WhiskerRuntime

public final class SafeAreaModule: Module {

    /// Live `NotificationCenter` observer token, held because
    /// `removeObserver(_:)` keys on identity and must be handed the same
    /// token back.
    private var observerToken: NSObjectProtocol?

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("SafeArea")
            Events("insetsChanged")

            OnStartObserving("insetsChanged") { [weak self] in
                self?.startObserving()
            }
            OnStopObserving("insetsChanged") { [weak self] in
                self?.stopObserving()
            }
        }
    }

    private func startObserving() {
        if observerToken != nil { return }

        observerToken = NotificationCenter.default.addObserver(
            forName: WhiskerView.safeAreaInsetsDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] note in
            guard let insets = note.userInfo?[WhiskerView.safeAreaInsetsKey]
                as? UIEdgeInsets else { return }
            self?.sendEvent("insetsChanged", Self.encode(insets))
        }

        // An already-laid-out WhiskerView fired
        // `safeAreaInsetsDidChange` before this observer existed, so its
        // current value has to be pushed by hand or the Rust signal sits
        // at `default()` until the next change.
        if let insets = currentAttachedInsets() {
            sendEvent("insetsChanged", Self.encode(insets))
        }
    }

    private func stopObserving() {
        if let token = observerToken {
            NotificationCenter.default.removeObserver(token)
        }
        observerToken = nil
    }

    /// Find the first attached `WhiskerView`'s safe-area insets among the
    /// connected scenes. `nil` on a cold start before first attach, where
    /// the regular notification path takes over once one mounts.
    private func currentAttachedInsets() -> UIEdgeInsets? {
        for scene in UIApplication.shared.connectedScenes {
            guard let windowScene = scene as? UIWindowScene else { continue }
            for window in windowScene.windows {
                if let view = findWhiskerView(in: window) {
                    return view.safeAreaInsets
                }
            }
        }
        return nil
    }

    /// Recursive search for the first `WhiskerView` in a view tree. An app
    /// with several of them gets the first in tree order; only the
    /// one-shot priming uses this, and the notification path handles the
    /// per-instance broadcast thereafter.
    private func findWhiskerView(in view: UIView) -> WhiskerView? {
        if let v = view as? WhiskerView { return v }
        for child in view.subviews {
            if let v = findWhiskerView(in: child) { return v }
        }
        return nil
    }

    /// `UIEdgeInsets` → `WhiskerValue.map` with the keys the Rust side's
    /// `decode_payload` expects. `UIEdgeInsets` is `left` / `right` while
    /// the payload is `leading` / `trailing`, mapped directly — this is
    /// LTR-only, and an RTL-aware version would consult
    /// `effectiveUserInterfaceLayoutDirection`.
    static func encode(_ insets: UIEdgeInsets) -> WhiskerValue {
        .map([
            "top": .float(Double(insets.top)),
            "leading": .float(Double(insets.left)),
            "trailing": .float(Double(insets.right)),
            "bottom": .float(Double(insets.bottom)),
        ])
    }
}
