// `<x-hello>` Whisker native element on iOS, in pure Swift.
//
// Phase 7-Φ.D migration target: replaces the previous Obj-C++
// `whisker_hello_element.mm` with a `@WhiskerElement`-annotated
// Swift class. The module-system path no longer relies on the
// `.mm`'s `+load`-time `LYNX_REGISTER_UI` macro — instead the
// `[ios].behaviors` entry in `whisker.module.toml` carries through
// to whisker-build's generated `WhiskerModuleBehaviors.swift`,
// which `AppDelegate.application(_:didFinishLaunchingWithOptions:)`
// invokes once at launch.
//
// The `@WhiskerElement("x-hello")` annotation itself is purely
// declarative in v1 — it injects a `__whiskerElementTag` constant
// for compile-time visibility, but the runtime registration call
// is generated from the manifest. v2 will drop the manifest entry
// once whisker-build parses macro applications via SwiftSyntax.
//
// Class name MUST match `[ios].behaviors[].class` in the manifest —
// `WhiskerModuleBehaviors.registerAll()` does
// `NSClassFromString("WhiskerHelloElement")`, and Swift's default
// Obj-C-runtime name for classes in a SwiftPM library is just the
// bare class name (no mangling) as long as the class is exposed
// to Obj-C. `LynxUI<UIView>` subclasses inherit from Lynx's own
// NSObject hierarchy so the Obj-C name matches automatically.

import UIKit
import Lynx
import WhiskerElements

@WhiskerElement("x-hello")
@objc(WhiskerHelloElement)
public final class WhiskerHelloElement: LynxUI<UIView> {
    // `@objc override` keeps the Swift override visible to Obj-C
    // dispatch — LynxUI's `createView` is an Obj-C method, so the
    // Lynx engine calls it via `objc_msgSend` rather than Swift's
    // vtable. Without `@objc`, Swift may inline the override only
    // for Swift-side callers and Lynx ends up invoking the parent
    // `LynxUI.createView` instead.
    @objc public override func createView() -> UIView {
        let v = UIView()
        // System pink to make the smoke test visually obvious —
        // if you see a pink rectangle, the tag-by-name dispatch +
        // module-system registration are working end-to-end.
        v.backgroundColor = .systemPink
        return v
    }
}
