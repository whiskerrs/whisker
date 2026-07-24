// `whisker-haptics` ModuleDefinition (iOS).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The SwiftPM codegen plugin discovers the
// `Module` subclass, emits a `@_cdecl` dispatch shim, and registers it
// so `whisker::platform_module::invoke("WhiskerHaptics", ...)` from Rust
// routes into these handlers.
//
// `UIImpactFeedbackGenerator`/`UISelectionFeedbackGenerator`/
// `UINotificationFeedbackGenerator` are the same three generators
// `expo-haptics` wraps — no permission entry needed (permission-free
// on iOS, unlike Android's `VIBRATE`). `prepare()` is called just
// before each trigger to minimize latency, matching Apple's own
// recommended usage pattern; generators are created fresh per call
// rather than cached, since holding one alive continuously would keep
// the Taptic Engine "warmed up" for no benefit here (calls are rare,
// user-initiated taps, not a rapid sequence).

import UIKit
import WhiskerModule // Module, ModuleDefinition, DSL

public final class HapticsModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerHaptics")

            // impact(style: "light" | "medium" | "heavy") -> Null
            Function("impact") { (args: [WhiskerValue]) -> WhiskerValue in
                let style = args.first?.asString ?? "light"
                let generator = UIImpactFeedbackGenerator(style: Self.impactStyle(style))
                generator.prepare()
                generator.impactOccurred()
                return .null
            }

            // selection() -> Null
            Function("selection") { (_: [WhiskerValue]) -> WhiskerValue in
                let generator = UISelectionFeedbackGenerator()
                generator.prepare()
                generator.selectionChanged()
                return .null
            }

            // notification(kind: "success" | "warning" | "error") -> Null
            Function("notification") { (args: [WhiskerValue]) -> WhiskerValue in
                let kind = args.first?.asString ?? "success"
                let generator = UINotificationFeedbackGenerator()
                generator.prepare()
                generator.notificationOccurred(Self.notificationType(kind))
                return .null
            }
        }
    }

    private static func impactStyle(_ raw: String) -> UIImpactFeedbackGenerator.FeedbackStyle {
        switch raw {
        case "heavy": return .heavy
        case "medium": return .medium
        default: return .light
        }
    }

    private static func notificationType(_ raw: String) -> UINotificationFeedbackGenerator.FeedbackType {
        switch raw {
        case "warning": return .warning
        case "error": return .error
        default: return .success
        }
    }
}
