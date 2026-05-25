// Phase L-3 — `whisker-hello-element` migrated to the new
// `ModuleDefinition` DSL on iOS.
//
// Replaces the pre-L-3 `@WhiskerComponent("Hello")`-annotated
// `WhiskerHelloComponent` class. The Lynx tag stays
// `whisker-hello-element:Hello` (cargo crate name namespace,
// prepended by the SwiftPM build plugin).
//
// Same shape as the Android side — `HelloModule.kt` in the
// adjacent `src/android/` directory.

import UIKit
import WhiskerModuleApi

/// Plain Lynx UI subclass. No annotation; instantiated by Lynx
/// via the behavior the DSL registers below.
///
/// `@objc(HelloView)` pins the Obj-C class name to the bare
/// `HelloView` (instead of `<SwiftPM-target>.HelloView`) so the
/// codegen plugin's `NSClassFromString` lookup can find it under
/// either name.
@objc(HelloView)
public final class HelloView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        let v = UIView()
        v.backgroundColor = .systemPink
        return v
    }
}

/// DSL-driven module. The SwiftPM codegen plugin (L-3 addition)
/// discovers this class by spotting the `: WhiskerModule`
/// inheritance, then emits a registration block that:
///
///   - Reads `definitionLazy.view!.viewClass` (== `HelloView`).
///   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
///     "whisker-hello-element:Hello")`.
///   - Calls `module.registerWithLynx()` so any Prop / Function
///     declared below installs via the Obj-C-runtime path (L-2b).
@objc(HelloModule)
public final class HelloModule: WhiskerModule {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Hello")
            View(HelloView.self) {
                // Hello is style-only (`Hello(style: "...")` on the
                // Rust side); no Prop / Function declarations needed.
                // Style props flow through Lynx's CSS path, not the
                // module API's Prop registration.
            }
        }
    }
}
