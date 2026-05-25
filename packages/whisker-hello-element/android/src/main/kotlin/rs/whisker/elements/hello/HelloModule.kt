// Phase L-3 — `whisker-hello-element` ModuleDefinition (Android).
//
// Replaces the pre-L-3 `@WhiskerComponent("Hello")`-annotated
// `WhiskerHelloComponent`. The Lynx tag stays
// `whisker-hello-element:Hello` (the cargo crate name is the
// namespace, prepended by the KSP processor).
//
// The `HelloView` Lynx UI subclass this references lives in
// `HelloView.kt`. Same split on iOS (`HelloModule.swift` +
// `HelloView.swift`).

package rs.whisker.elements.hello

import rs.whisker.annotations.WhiskerModule
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

/**
 * DSL-driven module. The KSP processor finds the `@WhiskerModule`
 * annotation and emits a registration call into
 * `WhiskerHelloElementBehaviors.registerAll()` that:
 *
 *   - Registers a `Behavior("whisker-hello-element:Hello")` whose
 *     `createUI` instantiates `HelloView(context)`.
 *   - Calls `module.registerWithLynx()` so any Prop / Function
 *     declared below installs via L-1's registration APIs.
 */
@WhiskerModule
class HelloModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Hello")
        View(HelloView::class.java) {
            // Hello is style-only (`Hello(style: "...")` on the
            // Rust side); no Prop / Function declarations needed.
            // Style props flow through Lynx's CSS path, not the
            // module API's Prop registration.
        }
    }
}
