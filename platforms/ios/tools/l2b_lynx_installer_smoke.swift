// Phase L-2b runtime smoke test for `WhiskerLynxInstaller`.
//
// Exercises the Obj-C-runtime install path end-to-end against a
// fake LynxUI subclass (just an NSObject), then uses Obj-C
// introspection (`class_respondsToSelector`,
// `class_getMethodImplementation`) to verify the expected
// selectors land on the class and dispatch correctly.
//
// Does **not** depend on the real Lynx framework — it stubs the
// LynxUI surface as plain NSObject. The point is to verify
// installer correctness in isolation; full Lynx-side dispatch
// will be exercised once the Phase L-3 sample-module migration
// drives the path through a real `whisker run`.
//
// ## Running
//
// ```
// swiftc -o /tmp/l2b_smoke \
//   platforms/ios/Sources/WhiskerModuleApi/ModuleDefinition.swift \
//   platforms/ios/Sources/WhiskerModuleApi/Module.swift \
//   platforms/ios/Sources/WhiskerModuleApi/WhiskerModuleRegistrar.swift \
//   platforms/ios/tools/l2b_lynx_installer_smoke.swift
// /tmp/l2b_smoke
// ```
//
// Or via the shell wrapper:
//
// ```
// platforms/ios/tools/run_l2b_smoke.sh
// ```

import Foundation
import ObjectiveC.runtime

// Fake LynxUI subclass — minimal NSObject we can install Obj-C
// methods on. Property side-effects let the test verify the
// installed selectors actually dispatch into the DSL closures.
@objc(SmokeFakeVideoView)
final class SmokeFakeVideoView: NSObject {
    @objc dynamic var lastSrc: String?
    @objc dynamic var playCount: Int = 0
}

final class SmokeModule: Module {
    override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("SmokeVideo")
            View(SmokeFakeVideoView.self) {
                Prop("src") { (view: SmokeFakeVideoView, value: String) in
                    view.lastSrc = value
                }
                Function("play") { (view: SmokeFakeVideoView) in
                    view.playCount += 1
                }
            }
        }
    }
}

@main
enum SmokeRunner {
    static func main() {
        runL2bSmoke()
    }
}

func runL2bSmoke() {
    let m = SmokeModule()
    m.registerWithLynx()

    // ---- 1. Class-method probe for the prop -------------------------
    //
    // Lynx's `LynxPropsProcessor` walks for `__lynx_prop_config__*`
    // class methods. We verify the metaclass responds, then call
    // the IMP and assert the returned config triple.

    let propProbeSel = NSSelectorFromString("__lynx_prop_config__src")
    let metaclass: AnyClass = object_getClass(SmokeFakeVideoView.self)!
    precondition(
        class_respondsToSelector(metaclass, propProbeSel),
        "metaclass missing __lynx_prop_config__src after install"
    )
    print("ok  class meta responds __lynx_prop_config__src")

    let probeIMP = class_getMethodImplementation(metaclass, propProbeSel)!
    typealias ProbeFn = @convention(c) (AnyClass, Selector) -> NSArray
    let probeFn = unsafeBitCast(probeIMP, to: ProbeFn.self)
    let triple = probeFn(SmokeFakeVideoView.self, propProbeSel) as! [String]
    precondition(
        triple == ["src", "setSrc", "id"],
        "probe triple wrong: \(triple)"
    )
    print("ok  __lynx_prop_config__src returns \(triple)")

    // ---- 2. Instance setter dispatch --------------------------------
    let setterSel = NSSelectorFromString("setSrc:requestReset:")
    precondition(
        class_respondsToSelector(SmokeFakeVideoView.self, setterSel),
        "instance missing setSrc:requestReset:"
    )
    print("ok  instance responds setSrc:requestReset:")

    let instance = SmokeFakeVideoView()
    let setterIMP = class_getMethodImplementation(SmokeFakeVideoView.self, setterSel)!
    typealias SetterFn = @convention(c) (AnyObject, Selector, NSString, Bool) -> Void
    let setterFn = unsafeBitCast(setterIMP, to: SetterFn.self)
    setterFn(instance, setterSel, "https://example.com/clip.mp4" as NSString, false)
    precondition(
        instance.lastSrc == "https://example.com/clip.mp4",
        "setter side effect missing: lastSrc = \(String(describing: instance.lastSrc))"
    )
    print("ok  setter side effect (lastSrc = \(instance.lastSrc!))")

    // ---- 3. Class-method probe for the function ---------------------
    let methodProbeSel = NSSelectorFromString("__lynx_ui_method_config__play")
    precondition(
        class_respondsToSelector(metaclass, methodProbeSel),
        "metaclass missing __lynx_ui_method_config__play"
    )
    print("ok  class meta responds __lynx_ui_method_config__play")

    let methodProbeIMP = class_getMethodImplementation(metaclass, methodProbeSel)!
    typealias MProbeFn = @convention(c) (AnyClass, Selector) -> NSString
    let methodProbeFn = unsafeBitCast(methodProbeIMP, to: MProbeFn.self)
    let methodKey = methodProbeFn(SmokeFakeVideoView.self, methodProbeSel) as String
    precondition(methodKey == "play", "method config returned \(methodKey)")
    print("ok  __lynx_ui_method_config__play returns \"\(methodKey)\"")

    // ---- 4. Instance method dispatch --------------------------------
    let methodSel = NSSelectorFromString("play:withResult:")
    precondition(
        class_respondsToSelector(SmokeFakeVideoView.self, methodSel),
        "instance missing play:withResult:"
    )
    print("ok  instance responds play:withResult:")

    var callbackInvoked = false
    let cb: @convention(block) (Int, AnyObject?) -> Void = { code, _ in
        precondition(code == 0, "expected success code 0, got \(code)")
        callbackInvoked = true
    }
    let methodIMP = class_getMethodImplementation(SmokeFakeVideoView.self, methodSel)!
    typealias MethodFn = @convention(c) (AnyObject, Selector, NSDictionary?, Any?) -> Void
    let methodFn = unsafeBitCast(methodIMP, to: MethodFn.self)
    let instance2 = SmokeFakeVideoView()
    let cbErased: Any = unsafeBitCast(cb, to: AnyObject.self)
    methodFn(instance2, methodSel, nil, cbErased)
    precondition(
        instance2.playCount == 1,
        "play handler didn't run: count = \(instance2.playCount)"
    )
    precondition(callbackInvoked, "callback wasn't invoked")
    print("ok  play handler ran (count=\(instance2.playCount)) + callback fired")

    print("")
    print("all L-2b runtime install assertions passed")
}
