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
// * `OnStartObserving("insetsChanged")` — register the
//   `NotificationCenter` observer **and** push the current insets of
//   any already-attached `WhiskerView` so the signal isn't stuck at
//   `default()` after a late subscription (e.g. a component that
//   mounts after the host view has finished laying out).
// * `OnStopObserving("insetsChanged")` — remove the observer. The
//   bridge guarantees this fires on the 1→0 transition, so we don't
//   leak the closure.

import Foundation
import UIKit
import WhiskerModule
import WhiskerRuntime

public final class SafeAreaModule: Module {

    /// Live `NotificationCenter` observer token. `nil` between the
    /// `OnStopObserving` removal and the next `OnStartObserving`
    /// install. Stored so the matching remove call targets the same
    /// token (`NotificationCenter.removeObserver(_:)` keys on
    /// identity).
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

        // Late-subscription priming: if a WhiskerView is already
        // attached and laid out, its `safeAreaInsetsDidChange` has
        // already fired (before our observer existed). Walk the
        // connected scenes and push the current value of any
        // matching key window's root so the Rust signal moves off
        // `default()` immediately.
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

    /// Find the first attached `WhiskerView`'s safe-area insets among
    /// the connected scenes. Returns `nil` if no `WhiskerView` is on
    /// screen yet (cold start before first attach) — the regular
    /// notification path takes over once one mounts.
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

    /// Recursive search for the first `WhiskerView` in a view tree.
    /// Linear in the number of subviews — fine for the one-shot
    /// startObserving priming. Apps with multiple WhiskerViews get the
    /// first one in tree order; the regular notification path
    /// thereafter handles the per-instance broadcast.
    private func findWhiskerView(in view: UIView) -> WhiskerView? {
        if let v = view as? WhiskerView { return v }
        for child in view.subviews {
            if let v = findWhiskerView(in: child) { return v }
        }
        return nil
    }

    /// `UIEdgeInsets` → `WhiskerValue.map` with the keys the Rust
    /// side's `decode_payload` expects. iOS's `UIEdgeInsets` uses
    /// `left` / `right` (not `leading` / `trailing`) — map directly,
    /// LTR-only for now. RTL-aware modules can read
    /// `effectiveUserInterfaceLayoutDirection` later if needed.
    static func encode(_ insets: UIEdgeInsets) -> WhiskerValue {
        .map([
            "top": .float(Double(insets.top)),
            "leading": .float(Double(insets.left)),
            "trailing": .float(Double(insets.right)),
            "bottom": .float(Double(insets.bottom)),
        ])
    }
}
