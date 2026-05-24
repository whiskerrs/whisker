// Whisker native element on iOS with local tag `Hello`. The Lynx
// registration string is `whisker-hello-element:Hello` — the
// SwiftPM build plugin prepends `context.package.displayName`
// (the cargo crate name) as the namespace so two unrelated
// packages can both declare a `Hello` element without colliding
// in Lynx's behaviour registry. Phase 7-Φ.H.2.
//
// Demonstrates the Whisker-only API surface (no `import Lynx`,
// no `LynxUI` mentions in author code).
//
// `WhiskerUI` / `WhiskerContext` are typealiases provided by
// WhiskerRuntime that resolve to the underlying Lynx types at
// the Swift type-system level. Stack traces / debugger views
// still show the real `LynxUI` class name — typealiases are a
// source-level concept only.

import UIKit
import WhiskerElements
import WhiskerRuntime

@WhiskerElement("Hello")
@objc(WhiskerHelloElement)
public final class WhiskerHelloElement: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        let v = UIView()
        v.backgroundColor = .systemPink
        return v
    }
}
