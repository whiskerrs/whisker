// `whisker-keyboard` Module (iOS).
//
// View-less module with two jobs:
//
//  * `keyboardChanged` event — while a Rust listener is registered,
//    observe the keyboard frame notifications and forward the keyboard's
//    overlap from the bottom of the screen (in points) as `{ height }`.
//    The Rust side (`packages/whisker-keyboard/src/lib.rs`) holds the
//    `RwSignal<f64>` `keyboard_height()` returns and updates it from
//    these events.
//
//  * `dismiss` function — a real global unfocus. `endEditing(true)` on
//    the key window walks the responder chain and resigns the current
//    first responder, so the keyboard goes down AND the field stops
//    receiving input (including from a hardware keyboard). This is not
//    the same as "hide the keyboard": on iOS the software keyboard's
//    presence is a consequence of first-responder status, so resigning
//    it is the correct, complete dismissal.
//
// ## Height semantics
//
// We report `max(0, screenHeight - keyboardFrameEnd.origin.y)` — the
// number of points the keyboard covers, measured from the bottom of the
// screen. That already includes the home-indicator safe area the
// keyboard sits over, so padding a bottom-anchored container by this
// value lifts its content exactly clear of the keyboard. On hide, the
// end frame sits fully off-screen and the value collapses to 0.

import Foundation
import UIKit
import WhiskerModule

public final class KeyboardModule: Module {

    /// Live notification observer tokens. Empty between the
    /// `OnStopObserving` removal and the next `OnStartObserving`.
    private var observers: [NSObjectProtocol] = []

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Keyboard")
            Events("keyboardChanged")

            OnStartObserving("keyboardChanged") { [weak self] in
                self?.startObserving()
            }
            OnStopObserving("keyboardChanged") { [weak self] in
                self?.stopObserving()
            }

            // Real global unfocus. Marshalled to the main thread because
            // `invoke` may dispatch the function body on the Lynx TASM
            // thread, and `endEditing` is UIKit work.
            Function("dismiss") { _ in
                DispatchQueue.main.async {
                    Self.keyWindow()?.endEditing(true)
                }
                return .null
            }
        }
    }

    private func startObserving() {
        if !observers.isEmpty { return }

        // `keyboardWillChangeFrame` covers show, hide, and interactive
        // height changes (e.g. an accessory bar appearing, the floating
        // iPad keyboard docking) in one path; `willHide` guarantees a
        // clean 0 even if a `changeFrame` is missed.
        let center = NotificationCenter.default
        observers.append(
            center.addObserver(
                forName: UIResponder.keyboardWillChangeFrameNotification,
                object: nil,
                queue: .main
            ) { [weak self] note in
                self?.emit(from: note)
            }
        )
        observers.append(
            center.addObserver(
                forName: UIResponder.keyboardWillHideNotification,
                object: nil,
                queue: .main
            ) { [weak self] _ in
                self?.sendEvent("keyboardChanged", .map(["height": .float(0)]))
            }
        )
    }

    private func stopObserving() {
        let center = NotificationCenter.default
        for token in observers {
            center.removeObserver(token)
        }
        observers.removeAll()
    }

    /// Convert a keyboard-frame notification into the covered height in
    /// points and dispatch it.
    private func emit(from note: Notification) {
        guard
            let frameEnd = (note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey]
                as? NSValue)?.cgRectValue
        else { return }

        // `keyboardFrameEnd` is in screen coordinates. Prefer the key
        // window's screen so multi-scene apps measure against the right
        // bounds; fall back to `UIScreen.main`.
        let screenHeight = Self.keyWindow()?.screen.bounds.height
            ?? UIScreen.main.bounds.height
        let covered = max(0, screenHeight - frameEnd.origin.y)
        sendEvent("keyboardChanged", .map(["height": .float(Double(covered))]))
    }

    /// The foreground-active key window across connected scenes.
    private static func keyWindow() -> UIWindow? {
        for scene in UIApplication.shared.connectedScenes {
            guard
                let windowScene = scene as? UIWindowScene,
                windowScene.activationState == .foregroundActive
            else { continue }
            if let key = windowScene.windows.first(where: { $0.isKeyWindow }) {
                return key
            }
        }
        // Fallback: any key window (cold start before a scene is marked
        // foreground-active).
        return UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap { $0.windows }
            .first { $0.isKeyWindow }
    }
}
