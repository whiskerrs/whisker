// Phase L-2b — iOS dispatch wiring for the `ModuleDefinition` DSL.
//
// At module-registration time (typically host-app launch), the
// framework calls `module.registerWithLynx()`. This walks the DSL
// definition the author built in `definition()` and installs the
// corresponding selectors on their `View(...)`-declared class via
// Obj-C runtime APIs:
//
//   - For each `Prop("foo") { setter }`:
//       + class method  `__lynx_prop_config__foo`        → `["foo", "setFoo", "id"]`
//       + instance method `setFoo:requestReset:`         → invokes the closure
//
//   - For each `Function("foo") { handler }`:
//       + class method  `__lynx_ui_method_config__foo`   → `"foo"`
//       + instance method `foo:withResult:`              → invokes the closure
//
// These are exactly the shapes Lynx's `LynxPropsProcessor` and
// `LynxUIMethodProcessor` scan for via class-method reflection, so
// the DSL-driven setters / methods become indistinguishable at the
// Lynx layer from those emitted by the `@WhiskerProp` /
// `@WhiskerUIMethod` annotations.
//
// ## Coexistence with the annotation API
//
// Both paths register against the same Lynx reflection surface.
// A class that uses *only* the DSL has no annotation-emitted
// methods to worry about. A class that mixes both works too — the
// emitted selectors are distinct (annotation-based methods are
// compile-time-known, DSL-based ones are runtime-added) and Lynx's
// reflection discovers both.
//
// ## Author bootstrap
//
// Module authors add their `WhiskerModule` subclass once to the
// host app's startup code (or — once Phase L-2's codegen plugin
// lands — let the build plugin generate the registration call). A
// minimal driver:
//
// ```swift
// @main
// struct App: SwiftUI.App {
//     init() {
//         VideoModule().registerWithLynx()
//     }
//     // ...
// }
// ```

import Foundation
import ObjectiveC.runtime

extension WhiskerModule {

    /// Walk `definitionLazy` and install the Lynx-visible setters /
    /// methods on the view class.
    ///
    /// Idempotent — a second call against the same view class
    /// re-installs over the previous registration (last-write-wins).
    /// Module-level (view-less) `Function`s are not yet wired
    /// up here; the module-level dispatch path stays on the
    /// `@WhiskerModule` annotation API through L-2b.
    public func registerWithLynx() {
        let def = self.definitionLazy

        // Function-only modules (no `View(...)` block) don't install
        // anything on a LynxUI class — their module-level `Function`s
        // dispatch through `dispatchModuleFunction(_:_:)` via the
        // codegen-emitted `@_cdecl` shim + `whisker_bridge_register_
        // module_dispatch` (Phase L-3), not through this method.
        guard let viewBlock = def.view else { return }

        let viewClass: AnyClass = viewBlock.viewClass

        for component in viewBlock.components {
            switch component {
            case let prop as WhiskerPropComponent:
                WhiskerLynxInstaller.installProp(prop, on: viewClass)
            case let fn as WhiskerFunctionComponent:
                WhiskerLynxInstaller.installFunction(fn, on: viewClass)
            case is WhiskerEventsComponent:
                // Events are declaration-only metadata in L-2a/b.
                // Dispatch is still via the imperative
                // `WhiskerCustomEvent.dispatch(from:name:params:)` path.
                continue
            default:
                continue
            }
        }
    }
}

// MARK: - Installer

/// Internal helper that materialises `WhiskerPropComponent` /
/// `WhiskerFunctionComponent` values as real Obj-C selectors on a
/// LynxUI subclass. Pulled into its own enum so the implementation
/// surface stays separate from the `WhiskerModule` public API.
internal enum WhiskerLynxInstaller {

    // ---- Prop installation ----------------------------------------------

    static func installProp(_ comp: WhiskerPropComponent, on viewClass: AnyClass) {
        // ---- 1. Class method `__lynx_prop_config__<name>` --------------
        //
        // Lynx's reflection: for each class method whose name starts
        // with `__lynx_prop_config__`, call it to recover the
        // `[propName, shortSelector, typeName]` triple.
        //
        // We pick selector `set<Capitalized>:requestReset:` so the
        // generated probe shape matches what `@WhiskerProp` would
        // have produced for an identically-named prop.
        let propName = comp.name
        let setterShort = "set" + propName.uppercasingFirstLetter()
        let configSelName = "__lynx_prop_config__" + propName

        // Block receives `(_cls: AnyClass)` because the IMP is
        // called with (self, SEL) — under
        // `imp_implementationWithBlock`, Swift's `self` slot drops
        // out, but Obj-C still passes the class. We accept it as
        // first arg and ignore.
        let configBlock: @convention(block) (AnyClass) -> [String] = { _ in
            // Type name `id` keeps Lynx's value-unboxing in
            // "pass through whatever the bridge handed in" mode.
            // L-2c will tighten this for typed props once the
            // DSL component carries explicit type info.
            return [propName, setterShort, "id"]
        }
        let configIMP = imp_implementationWithBlock(configBlock)
        addClassMethod(
            on: viewClass,
            selector: NSSelectorFromString(configSelName),
            imp: configIMP,
            // Method signature: `(NSArray *)method(id self, SEL _cmd)`
            //   '@' = return id, '@' = self id, ':' = SEL
            typeEncoding: "@@:"
        )

        // ---- 2. Instance method `set<Cap>:requestReset:` ---------------
        //
        // Two trailing args: the value (`id`) and a BOOL `requestReset`
        // flag Lynx uses to signal "the engine asked for a re-apply
        // of all props"; the DSL closure ignores it (the closure is
        // declarative — re-application is harmless).
        let setterSel = NSSelectorFromString(setterShort + ":requestReset:")
        let setter = comp.setter
        let setterBlock: @convention(block) (AnyObject, Any?, Bool) -> Void = { view, value, _ in
            setter(view, value)
        }
        let setterIMP = imp_implementationWithBlock(setterBlock)
        addInstanceMethod(
            on: viewClass,
            selector: setterSel,
            imp: setterIMP,
            // `v@:@B` = void return, (self id, SEL, value id, BOOL).
            typeEncoding: "v@:@B"
        )
    }

    // ---- Function installation ------------------------------------------

    static func installFunction(_ comp: WhiskerFunctionComponent, on viewClass: AnyClass) {
        // ---- 1. Class method `__lynx_ui_method_config__<name>` ----------
        let methodName = comp.name
        let configSelName = "__lynx_ui_method_config__" + methodName
        let configBlock: @convention(block) (AnyClass) -> NSString = { _ in
            return methodName as NSString
        }
        let configIMP = imp_implementationWithBlock(configBlock)
        addClassMethod(
            on: viewClass,
            selector: NSSelectorFromString(configSelName),
            imp: configIMP,
            // Returns NSString; `@@:` again.
            typeEncoding: "@@:"
        )

        // ---- 2. Instance method `<name>:withResult:` --------------------
        //
        // Signature: `-(void)<name>:(NSDictionary *)params
        //                       withResult:(LynxUIMethodCallbackBlock)cb`.
        //
        // The callback block is Lynx-typed `void(^)(NSInteger code,
        // id _Nullable data)`. Lynx's
        // `LynxUIMethodConstants.kUIMethodSuccess` is 0; we pass 0
        // on a normal return and pass back the result encoded as an
        // Any? (NSNull-erased on Void closures).
        //
        // Args decode: Lynx hands `params` as `@{ "args":
        //   [<positional entries>] }` (matching what the existing
        //   `@WhiskerUIMethod` macro decodes). We pull out the array
        //   and hand it to the closure.
        let methodSel = NSSelectorFromString(methodName + ":withResult:")
        let handler = comp.handler
        let methodBlock: @convention(block) (AnyObject, NSDictionary?, LynxUIMethodCallbackBlockShim?) -> Void = { view, params, cb in
            let rawArgs: [Any?] = {
                guard let p = params else { return [] }
                if let arr = p["args"] as? [Any?] { return arr }
                if let arr = p["args"] as? NSArray { return arr.map { $0 as Any? } }
                return []
            }()
            let result = handler(view, rawArgs)
            // Lynx kUIMethodSuccess = 0; we report 0 unconditionally
            // here since the closure-typed dispatch doesn't currently
            // surface failure codes. Type mismatches inside the
            // closure are silently swallowed by the type-erased
            // wrappers (with a debug-build log).
            cb?(0, result as AnyObject?)
        }
        let methodIMP = imp_implementationWithBlock(methodBlock)
        addInstanceMethod(
            on: viewClass,
            selector: methodSel,
            imp: methodIMP,
            // `v@:@@` = void return, (self id, SEL, params id, block id).
            // Obj-C blocks are typed as `@?` strictly, but `@` works
            // for argument-position blocks against `imp_implementation
            // WithBlock`-produced IMPs (the underlying ABI is the
            // same). `@?` here gives clearer crash diagnostics when
            // Lynx ever invokes us with a wrong shape.
            typeEncoding: "v@:@@?"
        )
    }

    // ---- Helpers ---------------------------------------------------------

    private static func addInstanceMethod(
        on cls: AnyClass,
        selector: Selector,
        imp: IMP,
        typeEncoding: String,
    ) {
        if !class_addMethod(cls, selector, imp, typeEncoding) {
            // Method already exists on the class — replace its IMP
            // in-place. `class_replaceMethod` returns the previous
            // IMP (or nil); we ignore that.
            _ = class_replaceMethod(cls, selector, imp, typeEncoding)
        }
    }

    private static func addClassMethod(
        on cls: AnyClass,
        selector: Selector,
        imp: IMP,
        typeEncoding: String,
    ) {
        // Class methods live on the metaclass.
        guard let metaclass = object_getClass(cls) else { return }
        if !class_addMethod(metaclass, selector, imp, typeEncoding) {
            _ = class_replaceMethod(metaclass, selector, imp, typeEncoding)
        }
    }
}

// MARK: - Lynx callback typealias

/// Local mirror of Lynx's `LynxUIMethodCallbackBlock`. We can't
/// import Lynx here without dragging the headers into the public
/// surface, so we redeclare the shape — Obj-C-block ABI matching
/// is what matters at runtime, not the Swift type identity.
///
/// Lynx's actual declaration is roughly:
///
/// ```objc
/// typedef void (^LynxUIMethodCallbackBlock)(NSInteger code, id _Nullable data);
/// ```
public typealias LynxUIMethodCallbackBlockShim =
    @convention(block) (Int, AnyObject?) -> Void

// MARK: - String helper

extension String {
    fileprivate func uppercasingFirstLetter() -> String {
        guard let first = self.first else { return self }
        return String(first).uppercased() + self.dropFirst()
    }
}
