// Plain Lynx UI subclass for the `Hello` element. No annotation;
// instantiated by Lynx via the behavior `HelloModule`'s
// `definition()` registers (see `HelloModule.swift`).
//
// `@objc(HelloView)` pins the Obj-C class name to the bare
// `HelloView` (instead of `<SwiftPM-target>.HelloView`) so the
// codegen plugin's `NSClassFromString` lookup can find it under
// either name.

import UIKit
import WhiskerModuleApi

@objc(HelloView)
public final class HelloView: WhiskerUI<UIView> {
    @objc public override func createView() -> UIView {
        let v = UIView()
        v.backgroundColor = .systemPink
        return v
    }
}
