// Phase L-2a — `ModuleDefinition` DSL surface (iOS).
//
// Single-language DSL that supersedes the `@WhiskerComponent` /
// `@WhiskerProp` / `@WhiskerUIMethod` annotation set. Modeled after
// Expo Modules' `ModuleDefinition` (https://docs.expo.dev/modules/module-api/)
// but emits direct registrations against Lynx's prop / method
// dispatch tables instead of routing through `@LynxProp` /
// `@LynxUIMethod` reflection.
//
// ## Target syntax
//
// ```swift
// @WhiskerModule
// public final class VideoModule: Module {
//     public override func definition() -> ModuleDefinition {
//         Name("Video")
//
//         Constants(["maxResolution": "1080p"])
//
//         View(WhiskerVideoView.self) {
//             Prop("src") { (view: WhiskerVideoView, value: String) in
//                 view.setSrc(value)
//             }
//             Function("play")  { (view: WhiskerVideoView) in view.play()  }
//             Function("pause") { (view: WhiskerVideoView) in view.pause() }
//             Function("seek")  { (view: WhiskerVideoView, seconds: Double) in
//                 view.seek(seconds)
//             }
//             Events("onCompleted")
//         }
//     }
// }
// ```
//
// View-less modules (function-only) work the same way without the
// inner `View(...)` block:
//
// ```swift
// @WhiskerModule
// public final class LocalStoreModule: Module {
//     public override func definition() -> ModuleDefinition {
//         Name("WhiskerLocalStore")
//         Function("save") { (key: String, value: String) -> Bool in
//             UserDefaults.standard.set(value, forKey: key)
//             return true
//         }
//         Function("load") { (key: String) -> String? in
//             UserDefaults.standard.string(forKey: key)
//         }
//     }
// }
// ```
//
// ## What L-2a delivers
//
// This file defines the **DSL surface and value model**. The
// `Module` base class collects the `ModuleDefinition` at init time;
// the iOS dispatch glue (L-2b) + the `WhiskerComponentsCodegen`
// plugin's `@WhiskerModule` discovery wire it into Lynx's prop /
// method dispatch tables.

import Foundation

// MARK: - Component model

/// Type-erased component the result builder collects. Every DSL
/// factory function returns one of these. Concrete variants live
/// below.
public protocol WhiskerDefinitionComponent {}

// MARK: - Top-level components

/// Module-name component. Exactly one expected per module.
///
/// `Name("Video")` registers the module under the local tag string
/// `Video`; the Whisker build layer prepends the package's cargo
/// crate name to produce the fully-qualified tag
/// (`<crate>:Video`) so two crates can both export a `Video`
/// element without colliding.
public struct WhiskerNameComponent: WhiskerDefinitionComponent {
    public let value: String
    public init(_ value: String) { self.value = value }
}

/// Static constants component. Authors emit a dictionary that
/// the framework exposes to the host. Mirrors Expo's deprecated
/// `Constants([...])` form — we ship the dictionary form only;
/// the dynamic closure form (`Constants { [...] }`) and per-key
/// `Constant("key") { ... }` lazy form can land later.
public struct WhiskerConstantsComponent: WhiskerDefinitionComponent {
    public let values: [String: Any]
    public init(_ values: [String: Any]) { self.values = values }
}

/// View block — registers a Lynx UI subclass + its `Prop` /
/// `Function` / `Events` sub-components. Exactly one expected for
/// view-bearing modules; absent entirely for function-only modules.
public struct WhiskerViewComponent: WhiskerDefinitionComponent {
    /// Type-erased to AnyClass so the call site doesn't need to
    /// generic the parent struct. The concrete class is the Lynx
    /// UI subclass (typically a `LynxUI<UIView>` subclass).
    public let viewClass: AnyClass
    public let components: [WhiskerDefinitionComponent]
    public init(viewClass: AnyClass, components: [WhiskerDefinitionComponent]) {
        self.viewClass = viewClass
        self.components = components
    }
}

// MARK: - View-block components (also legal at module-level for function-only modules)

/// Type-erased setter the framework calls on prop dispatch.
/// `view` is the Lynx UI instance the value was set on; `value`
/// is the raw Lynx value (Whisker value-decoded by L-2b/c).
public typealias WhiskerPropSetter = (_ view: AnyObject, _ value: Any?) -> Void

/// Prop component — a single named setter on the view class.
public struct WhiskerPropComponent: WhiskerDefinitionComponent {
    public let name: String
    public let setter: WhiskerPropSetter
    public init(name: String, setter: @escaping WhiskerPropSetter) {
        self.name = name
        self.setter = setter
    }
}

/// Type-erased function handler. `args` carry through positional
/// arguments from the JS / Rust call site; the result is either
/// the function's return value (auto-encoded) or `nil` for
/// `Void`-returning bodies. `view` is `nil` for module-level
/// `Function`s, the Lynx UI instance for view-block `Function`s.
public typealias WhiskerFunctionHandler = (_ view: AnyObject?, _ args: [Any?]) -> Any?

/// Sync function component. Same shape inside a `View(...)`
/// block (gets the view as `view`) and at module-level
/// (view-less; `view` is `nil`).
public struct WhiskerFunctionComponent: WhiskerDefinitionComponent {
    public let name: String
    public let handler: WhiskerFunctionHandler
    public init(name: String, handler: @escaping WhiskerFunctionHandler) {
        self.name = name
        self.handler = handler
    }
}

/// Event-name declaration component. Authors declare event names
/// they intend to emit; the runtime uses the list for
/// type-checking + future docs generation. The dispatch site
/// stays imperative (`WhiskerCustomEvent.dispatch(from:name:params:)`)
/// — `Events("foo")` just records the name.
public struct WhiskerEventsComponent: WhiskerDefinitionComponent {
    public let names: [String]
    public init(_ names: [String]) { self.names = names }
}

// MARK: - Result builders

/// Top-level `definition() -> ModuleDefinition` body builder.
@resultBuilder
public struct WhiskerModuleDefinitionBuilder {
    public static func buildBlock(_ components: WhiskerDefinitionComponent...) -> ModuleDefinition {
        ModuleDefinition(components: components)
    }
    /// Allow a body that ends with a single component (no trailing
    /// expressions) — Swift's variadic packing covers it but the
    /// explicit overload makes the diagnostic clearer when there's
    /// only one entry.
    public static func buildBlock(_ component: WhiskerDefinitionComponent) -> ModuleDefinition {
        ModuleDefinition(components: [component])
    }
    /// Empty body — yields an empty definition. Mostly used by the
    /// `WhiskerModule.definition()` default impl below.
    public static func buildBlock() -> ModuleDefinition {
        ModuleDefinition(components: [])
    }

    /// Optional / conditional emission.
    public static func buildOptional(
        _ component: WhiskerDefinitionComponent?
    ) -> WhiskerDefinitionComponent {
        component ?? EmptyComponent()
    }
    public static func buildEither(
        first: WhiskerDefinitionComponent
    ) -> WhiskerDefinitionComponent { first }
    public static func buildEither(
        second: WhiskerDefinitionComponent
    ) -> WhiskerDefinitionComponent { second }
}

/// Nested `View(...) { ... }` body builder.
@resultBuilder
public struct WhiskerViewDefinitionBuilder {
    public static func buildBlock(
        _ components: WhiskerDefinitionComponent...
    ) -> [WhiskerDefinitionComponent] {
        components
    }
    public static func buildBlock(
        _ component: WhiskerDefinitionComponent
    ) -> [WhiskerDefinitionComponent] {
        [component]
    }
    public static func buildBlock() -> [WhiskerDefinitionComponent] { [] }

    public static func buildOptional(
        _ component: WhiskerDefinitionComponent?
    ) -> WhiskerDefinitionComponent {
        component ?? EmptyComponent()
    }
    public static func buildEither(
        first: WhiskerDefinitionComponent
    ) -> WhiskerDefinitionComponent { first }
    public static func buildEither(
        second: WhiskerDefinitionComponent
    ) -> WhiskerDefinitionComponent { second }
}

/// Empty / placeholder component for optional-builder paths.
/// Registration phase filters these out.
public struct EmptyComponent: WhiskerDefinitionComponent {
    public init() {}
}

// MARK: - ModuleDefinition value

/// The assembled definition the framework registers with Lynx at
/// module-init time. Immutable; collected from the
/// `@WhiskerModuleDefinitionBuilder` body of `definition()`.
public struct ModuleDefinition {
    public let components: [WhiskerDefinitionComponent]

    public init(components: [WhiskerDefinitionComponent]) {
        self.components = components
    }

    public init(@WhiskerModuleDefinitionBuilder _ body: () -> ModuleDefinition) {
        self = body()
    }

    /// Module name. Returns `nil` if no `Name(...)` was declared —
    /// Phase L-2b will require this and surface a clear error at
    /// registration time.
    public var name: String? {
        for c in components {
            if let n = c as? WhiskerNameComponent { return n.value }
        }
        return nil
    }

    /// View block, if any.
    public var view: WhiskerViewComponent? {
        for c in components {
            if let v = c as? WhiskerViewComponent { return v }
        }
        return nil
    }

    /// Constants dictionary merged from all `Constants(...)` blocks.
    public var constants: [String: Any] {
        var merged: [String: Any] = [:]
        for c in components {
            if let cc = c as? WhiskerConstantsComponent {
                for (k, v) in cc.values { merged[k] = v }
            }
        }
        return merged
    }

    /// Module-level (view-less) functions — i.e. `Function(...)`
    /// declared OUTSIDE a `View(...)` block.
    public var functions: [WhiskerFunctionComponent] {
        components.compactMap { $0 as? WhiskerFunctionComponent }
    }
}

// MARK: - Top-level factories — the DSL surface authors call

// Naming convention: PascalCase to mirror Expo Modules. Swift
// allows top-level function names of any case.

/// `Name("Foo")` — the module's local tag name.
public func Name(_ value: String) -> WhiskerDefinitionComponent {
    WhiskerNameComponent(value)
}

/// `Constants([key: value, ...])` — static constants exposed to
/// the host. Dictionary form only in v1; per-key lazy form
/// (`Constant("k") { ... }`) lands later.
public func Constants(_ values: [String: Any]) -> WhiskerDefinitionComponent {
    WhiskerConstantsComponent(values)
}

/// `View(MyView.self) { ... }` — registers a Lynx UI subclass +
/// its inner DSL block (Prop / Function / Events).
public func View<V: AnyObject>(
    _ viewClass: V.Type,
    @WhiskerViewDefinitionBuilder _ body: () -> [WhiskerDefinitionComponent]
) -> WhiskerDefinitionComponent {
    WhiskerViewComponent(viewClass: viewClass, components: body())
}

/// `Events("a", "b", ...)` — declare event names this module
/// emits. Just metadata; dispatch stays imperative via
/// `WhiskerCustomEvent.dispatch(...)`.
public func Events(_ names: String...) -> WhiskerDefinitionComponent {
    WhiskerEventsComponent(names)
}

// MARK: - Prop factories

/// `Prop("foo") { (view, value) in ... }` — view-bearing prop
/// setter. The closure's parameter types are checked at the call
/// site; the framework runs a downcast at dispatch time and
/// silently no-ops on a mismatch (with a debug-build log).
public func Prop<V: AnyObject, T>(
    _ name: String,
    _ setter: @escaping (V, T) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerPropComponent(name: name) { uiAny, valueAny in
        guard let ui = uiAny as? V else {
            #if DEBUG
            print("WhiskerModuleApi: Prop(\"\(name)\") view type mismatch — expected \(V.self), got \(type(of: uiAny))")
            #endif
            return
        }
        guard let value = valueAny as? T else {
            #if DEBUG
            print("WhiskerModuleApi: Prop(\"\(name)\") value type mismatch — expected \(T.self), got \(type(of: valueAny as Any))")
            #endif
            return
        }
        setter(ui, value)
    }
}

// MARK: - Function factories — 0-arg through 4-arg overloads

// L-2a ships sync `Function` only. `AsyncFunction` lands in L-2d.

/// `Function("noargs") { (view) in ... }` — sync view-bound
/// 0-arg function.
public func Function<V: AnyObject>(
    _ name: String,
    _ handler: @escaping (V) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, _ in
        guard let view = viewAny as? V else { return nil }
        handler(view)
        return nil
    }
}

public func Function<V: AnyObject, R>(
    _ name: String,
    _ handler: @escaping (V) -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, _ in
        guard let view = viewAny as? V else { return nil }
        return handler(view)
    }
}

public func Function<V: AnyObject, A>(
    _ name: String,
    _ handler: @escaping (V, A) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, args in
        guard let view = viewAny as? V, let a = args.first as? A else { return nil }
        handler(view, a)
        return nil
    }
}

public func Function<V: AnyObject, A, R>(
    _ name: String,
    _ handler: @escaping (V, A) -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, args in
        guard let view = viewAny as? V, let a = args.first as? A else { return nil }
        return handler(view, a)
    }
}

public func Function<V: AnyObject, A, B>(
    _ name: String,
    _ handler: @escaping (V, A, B) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, args in
        guard let view = viewAny as? V,
              args.count >= 2,
              let a = args[0] as? A, let b = args[1] as? B
        else { return nil }
        handler(view, a, b)
        return nil
    }
}

public func Function<V: AnyObject, A, B, R>(
    _ name: String,
    _ handler: @escaping (V, A, B) -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, args in
        guard let view = viewAny as? V,
              args.count >= 2,
              let a = args[0] as? A, let b = args[1] as? B
        else { return nil }
        return handler(view, a, b)
    }
}

// Module-level (view-less) `Function` — same shape but no view
// arg. Reachable inside a function-only module.

public func Function(
    _ name: String,
    _ handler: @escaping () -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, _ in
        handler()
        return nil
    }
}

public func Function<R>(
    _ name: String,
    _ handler: @escaping () -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, _ in
        handler()
    }
}

public func Function<A>(
    _ name: String,
    _ handler: @escaping (A) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, args in
        guard let a = args.first as? A else { return nil }
        handler(a)
        return nil
    }
}

public func Function<A, R>(
    _ name: String,
    _ handler: @escaping (A) -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, args in
        guard let a = args.first as? A else { return nil }
        return handler(a)
    }
}

public func Function<A, B>(
    _ name: String,
    _ handler: @escaping (A, B) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, args in
        guard args.count >= 2, let a = args[0] as? A, let b = args[1] as? B else { return nil }
        handler(a, b)
        return nil
    }
}

public func Function<A, B, R>(
    _ name: String,
    _ handler: @escaping (A, B) -> R
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, args in
        guard args.count >= 2, let a = args[0] as? A, let b = args[1] as? B else { return nil }
        return handler(a, b)
    }
}
