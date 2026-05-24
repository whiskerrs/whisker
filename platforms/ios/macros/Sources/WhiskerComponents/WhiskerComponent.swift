// Whisker module-system iOS attached macro — `@WhiskerComponent`.
//
// Applied to a `WhiskerUI<View>` subclass to mark it as the
// platform UI class for a given tag name:
//
// ```swift
// import WhiskerComponents
// import WhiskerRuntime
// import UIKit
//
// @WhiskerComponent("Hello")
// public class WhiskerHelloComponent: WhiskerUI<UIView> {
//     public override func createView() -> UIView {
//         let v = UIView()
//         v.backgroundColor = .systemPink
//         return v
//     }
// }
// ```
//
// The tag string the author passes here is the *local* name. The
// `WhiskerComponentsCodegen` SwiftPM build plugin prepends the
// SwiftPM package's `displayName` (which equals the cargo crate
// name for Whisker module packages) at codegen time to produce
// the fully-qualified registration string
// `<crate-name>:<local-tag>` — so two unrelated module packages
// can both declare a `Hello` element without colliding in Lynx's
// behaviour registry. Phase 7-Φ.H.2.
//
// The macro decorates the annotated class with `@objc(<ClassName>)`
// so the Obj-C runtime name is stable (no Swift name mangling),
// allowing the codegen to resolve the class via
// `NSClassFromString` regardless of SwiftPM target mangling.

/// Marks a `WhiskerUI` subclass as a Whisker platform component with
/// local tag name `tag`.
///
/// Apply to any subclass of `WhiskerUI<View>` (or a Lynx-provided
/// flatten variant) declared in a Whisker module crate's
/// `swift_sources`. The `WhiskerComponentsCodegen` SwiftPM build
/// plugin discovers the annotation, prepends the crate name as a
/// namespace (`<crate-name>:<tag>`), and emits the
/// `LynxComponentRegistry.registerUI(cls, withName:)` call into
/// the package's generated registration helper.
@attached(member, names: arbitrary)
@attached(memberAttribute)
public macro WhiskerComponent(_ tag: String) =
    #externalMacro(module: "WhiskerComponentsMacros", type: "WhiskerComponentMacro")

/// Marks a class as a Whisker platform module under `name`.
///
/// Apply to a class whose instance methods follow the shape
/// `func name(_ args: [WhiskerValue]) -> WhiskerValue` — that's
/// the wire contract the C bridge dispatches against. The class
/// must have a zero-arg initialiser (`init()`); a fresh instance
/// is constructed per dispatch.
///
/// The macro expands into a top-level `@_cdecl` C-callable
/// dispatch shim that switches on the incoming method name and
/// routes to the matching instance method. The
/// `WhiskerComponentsCodegen` SwiftPM build-tool plugin discovers
/// the annotation at build time and emits a
/// `whisker_bridge_register_module_dispatch(name, _whiskerDispatch_…)`
/// call into `WhiskerModuleBehaviors.swift` so the C bridge's
/// by-name lookup finds the shim at runtime.
///
/// Pairs with the Rust-side `#[whisker::platform_module]` proc macro
/// (Phase 7-Φ.E.5) — the Swift class provides the platform-side
/// implementation, the Rust proxy provides the typed call surface.
/// Authors are encouraged to hand-write a separate typed wrapper
/// in front of the proc-macro-emitted unit struct rather than
/// publish the `[WhiskerValue]`-shaped API directly.
///
/// ```swift
/// import WhiskerComponents
/// import WhiskerRuntime  // brings WhiskerValue + WhiskerValueRaw into scope
///
/// @WhiskerModule("WhiskerStorage")
/// public class WhiskerStorageImpl {
///     func save(_ args: [WhiskerValue]) -> WhiskerValue {
///         guard args.count >= 2,
///               case .string(let key)   = args[0],
///               case .string(let value) = args[1] else {
///             return .bool(false)
///         }
///         UserDefaults.standard.set(value, forKey: key)
///         return .bool(true)
///     }
/// }
/// ```
///
/// Phase 7-Φ.F: dispatch shim replaces the previous Obj-C
/// `NSInvocation`-based registry. The class no longer needs to
/// inherit from `NSObject` or declare `@objc` selectors.
// `names: prefixed(...)` covers both peer functions the macro
// emits:
//   - `_whiskerDispatch_<ClassName>` — the @_cdecl C dispatch shim
//   - `_whiskerRegister_<ClassName>` — a tiny registration helper
//     that calls `whisker_bridge_register_module_dispatch`. Lives
//     in the same .o as the dispatch shim so Swift can convert
//     the dispatch fn to `@convention(c)` locally (no inter-.o
//     thunk → no duplicate-symbol linker error in Debug). The
//     codegen plugin calls `_whiskerRegister_<ClassName>()` rather
//     than taking the dispatch shim's address directly.
//
// Swift rejects `names: arbitrary` on peer macros at global scope
// (a macro could otherwise shadow any top-level decl), so we
// enumerate each prefix explicitly.
@attached(peer, names: prefixed(_whiskerDispatch_), prefixed(_whiskerRegister_))
public macro WhiskerModule(_ name: String) =
    #externalMacro(module: "WhiskerComponentsMacros", type: "WhiskerModuleMacro")

/// Marks a `WhiskerUI` subclass's method as a UI method invokable
/// from Rust via an `ElementRef<T>`. Phase 7-Φ.H.2.
///
/// Apply to an instance method whose shape matches the
/// `@WhiskerModule` dispatch contract:
/// `func name(_ args: [WhiskerValue]) -> WhiskerValue`. The macro
/// emits two peer declarations:
///
///   1. `@objc class func __lynx_ui_method_config__<name>() -> String`
///      — Lynx's `LynxUIMethodProcessor` reflection probe.
///   2. `@objc func <name>(_ params: NSDictionary?, withResult: …)`
///      — the actual dispatch entry point Lynx calls. It decodes
///      the params NSDictionary back to `[WhiskerValue]`, invokes
///      the user method via `self.<name>(args)`, then encodes the
///      returned `WhiskerValue` for Lynx's callback.
///
/// ```swift
/// @WhiskerComponent("Video")
/// public class WhiskerVideoComponent: WhiskerUI<UIView> {
///     @WhiskerUIMethod
///     public func play(_ args: [WhiskerValue]) -> WhiskerValue {
///         (view as? VideoView)?.play()
///         return .null
///     }
/// }
/// ```
///
/// User code never mentions `LYNX_UI_METHOD`, `LynxUIMethodProcessor`,
/// or `NSDictionary` — Lynx symbol hiding (Phase 7-Φ.H.1) extends
/// to the UI-method path.
///
/// `WhiskerUIMethod` is reserved for element-side method
/// declarations. `WhiskerMethod` (without the `UI` prefix) is
/// reserved for future module-side method declarations.
@attached(peer, names: arbitrary)
public macro WhiskerUIMethod() =
    #externalMacro(module: "WhiskerComponentsMacros", type: "WhiskerUIMethodMacro")

/// Marks an `@objc` method on a `@WhiskerComponent` class as the
/// setter for a Lynx prop named `name`. Phase 7-Φ.H.2 follow-up.
///
/// Sibling of the Android `@WhiskerProp` annotation (Phase
/// 7-Φ.H.1.b). On both platforms the macro / KSP processor wires
/// the user's Whisker-shaped setter to Lynx's underlying reflection-
/// based prop dispatch so module authors don't need to import
/// `LYNX_PROP_SETTER` / `@LynxProp` directly.
///
/// ```swift
/// @WhiskerComponent("Video")
/// public class WhiskerVideoComponent: WhiskerUI<UIView> {
///     @WhiskerProp("src")
///     @objc public func setSrc(_ value: NSString, requestReset: Bool) {
///         // load URL
///     }
/// }
/// ```
///
/// The annotated method MUST:
///   - Be `@objc` (Lynx walks the Obj-C runtime for reflection).
///   - Take exactly `(value: T, requestReset: Bool)` where T is the
///     prop's value type. Lynx's PropsProcessor builds the selector
///     `<methodName>:requestReset:` and invokes it with that shape.
@attached(peer, names: arbitrary)
public macro WhiskerProp(_ name: String) =
    #externalMacro(module: "WhiskerComponentsMacros", type: "WhiskerPropMacro")
