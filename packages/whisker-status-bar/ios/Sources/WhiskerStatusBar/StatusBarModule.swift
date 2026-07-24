// `whisker-status-bar` ModuleDefinition (iOS).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The SwiftPM codegen plugin discovers the
// `Module` subclass, emits a `@_cdecl` dispatch shim, and registers it
// so `whisker::platform_module::invoke("WhiskerStatusBar", ...)` from
// Rust routes into these handlers.
//
// iOS decides status-bar appearance per view controller by default, so
// these app-level `UIApplication` setters are no-ops UNLESS
// `UIViewControllerBasedStatusBarAppearance = false` is set in
// Info.plist — which this crate's plugin injects (see `src/plugin.rs`).
// The setters are deprecated (since iOS 9) but remain fully functional
// with that plist key, and are the only way for a view-less module to
// drive the status bar without owning a view controller. This mirrors
// what `expo-status-bar` does on iOS.
//
// All calls hop to the main thread — UIKit status-bar mutation is main-
// thread-only, and a Rust caller may invoke from any thread.

import UIKit
import WhiskerModule

public final class StatusBarModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerStatusBar")

            // setHidden(hidden: Bool) -> Null
            Function("setHidden") { (args: [WhiskerValue]) -> WhiskerValue in
                let hidden = args.first?.asBool ?? false
                DispatchQueue.main.async {
                    UIApplication.shared.setStatusBarHidden(hidden, with: .fade)
                }
                return .null
            }

            // setStyle(style: "light" | "dark") -> Null
            Function("setStyle") { (args: [WhiskerValue]) -> WhiskerValue in
                let style: UIStatusBarStyle =
                    (args.first?.asString == "light") ? .lightContent : .darkContent
                DispatchQueue.main.async {
                    UIApplication.shared.setStatusBarStyle(style, animated: true)
                }
                return .null
            }
        }
    }
}
