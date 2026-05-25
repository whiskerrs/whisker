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

/// Marks a `Module` subclass as a Whisker module the build should
/// register.
///
/// Applying `@WhiskerModule` is the registration trigger — the
/// `WhiskerComponentsCodegen` SwiftPM build-tool plugin scans the
/// target's sources for it and emits the Lynx behaviour /
/// module-dispatch registration into `<Target>+Generated.swift`.
/// The module's local tag / name comes from the `Name("…")` entry
/// inside `definition()`, so the annotation itself takes no
/// arguments. It plays the role of Expo's
/// `expo-module.config.json` entry, but inline at the declaration
/// (idiomatic Swift: an attribute marks the entry point, like
/// `@main`).
///
/// ```swift
/// import WhiskerComponents   // @WhiskerModule
/// import WhiskerModuleApi    // Module, ModuleDefinition, DSL
///
/// @WhiskerModule
/// public final class VideoModule: Module {
///     public override func definition() -> ModuleDefinition {
///         Name("Video")
///         View(VideoView.self) {
///             Prop("src") { (view: VideoView, value: String) in view.setSrc(value) }
///             Function("play") { (view: VideoView) in view.play() }
///         }
///     }
/// }
/// ```
///
/// Companion of Android's `@WhiskerModule` (KSP). The macro itself
/// expands to nothing — it's a pure marker; the codegen plugin
/// does the registration work by scanning for the attribute.
///
/// Declared as a `member` macro (not `peer`) so it's valid on a
/// top-level class: Swift forbids `peer` macros that introduce
/// `arbitrary` names at global scope, but a `member` macro's names
/// live inside the type's scope. The macro produces no members —
/// the role is just a vehicle for a valid marker attribute.
@attached(member, names: arbitrary)
public macro WhiskerModule() =
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
