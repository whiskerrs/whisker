// Phase L-3 — `whisker-hello-element` ModuleDefinition (iOS).
//
// Replaces the pre-L-3 `@WhiskerComponent("Hello")`-annotated
// `WhiskerHelloComponent`. The Lynx tag stays
// `whisker-hello-element:Hello` (cargo crate name namespace,
// prepended by the SwiftPM build plugin).
//
// The `HelloView` Lynx UI subclass this references lives in
// `HelloView.swift`. Same split on Android (`HelloModule.kt` +
// `HelloView.kt`).

import WhiskerModuleApi

/// DSL-driven module. The SwiftPM codegen plugin (L-3) discovers
/// this class by spotting the `: WhiskerModule` inheritance, then
/// emits a registration block that:
///
///   - Reads `definitionLazy.view!.viewClass` (== `HelloView`).
///   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
///     "whisker-hello-element:Hello")`.
///   - Calls `module.registerWithLynx()` so any Prop / Function
///     declared below installs via the Obj-C-runtime path (L-2b).
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
