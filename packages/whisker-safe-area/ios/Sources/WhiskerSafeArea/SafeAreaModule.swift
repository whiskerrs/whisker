// `whisker-safe-area` Module (iOS).
//
// View-less module. Subscribes to `WhiskerInsetsDispatcher` once at
// least one Rust listener is registered against the `insetsChanged`
// event and dispatches its `UIEdgeInsets` as a WhiskerValue payload.
//
// The Rust side (`packages/whisker-safe-area/src/lib.rs`) is the only
// consumer — it holds the `RwSignal<SafeAreaInsets>` `safe_area_insets()`
// returns and updates it from this module's events.
//
// ## Lifecycle
//
// `addListener` immediately pushes the current insets of an already
// attached `WhiskerView`, so late subscribers do not stay at `default()`.
// The bridge guarantees `OnStopObserving` fires on the 1→0 transition,
// so the observer closure can't leak.

import Foundation
import UIKit
import WhiskerModule

@WhiskerModule
public final class SafeAreaModule: Module {

    /// Live dispatcher subscription, released on the 1→0 transition.
    private var registration: WhiskerInsetsDispatcher.Registration?

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
        if registration != nil { return }
        registration = WhiskerInsetsDispatcher.addListener { [weak self] insets in
            self?.sendEvent("insetsChanged", Self.encode(insets))
        }
    }

    private func stopObserving() {
        if let registration { WhiskerInsetsDispatcher.removeListener(registration) }
        registration = nil
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
