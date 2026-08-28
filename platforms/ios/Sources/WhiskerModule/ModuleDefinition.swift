// `ModuleDefinition` DSL surface (iOS).
//
// Modeled after Expo Modules' `ModuleDefinition`
// (https://docs.expo.dev/modules/module-api/) but emits direct Host
// registrations without a renderer SDK dependency.
//
// ## Syntax
//
// ```swift
// @WhiskerModule
// public final class VideoModule: Module {
//     public override func definition() -> ModuleDefinition {
//         Name("whisker-video:Video")
//
//         View("whisker-video:Video", WhiskerVideoView.self) {
//             Prop("src") { (view: WhiskerVideoView, value: String) in
//                 view.setSrc(value)
//             }
//             Command("play")  { (view: WhiskerVideoView, _: WhiskerValue) in view.play()  }
//             Command("pause") { (view: WhiskerVideoView, _: WhiskerValue) in view.pause() }
//             Command("seek")  { (view: WhiskerVideoView, value: WhiskerValue) in
//                 guard case let .float(seconds) = value else { return }
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
// The `Module` base class collects the `ModuleDefinition` at init
// time; the `WhiskerModuleCodegen` plugin's `@WhiskerModule`
// discovery wires it into the generated Host registry.

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

/// View block — registers a native View class + its `Prop` /
/// `Function` / `Events` sub-components. Exactly one expected for
/// view-bearing modules; absent entirely for function-only modules.
public struct WhiskerViewComponent: WhiskerDefinitionComponent {
    /// Type-erased to AnyClass so the call site doesn't need to
    /// generic the parent struct. The concrete class is a Host View.
    public let viewClass: AnyClass?
    let factory: WhiskerElementFactory?
    public let components: [WhiskerDefinitionComponent]
    public let elementName: String?
    public init(
        viewClass: AnyClass,
        components: [WhiskerDefinitionComponent],
        elementName: String? = nil
    ) {
        self.viewClass = viewClass
        self.factory = nil
        self.components = components
        self.elementName = elementName
    }

    init(factory: WhiskerElementFactory, components: [WhiskerDefinitionComponent]) {
        self.viewClass = nil
        self.factory = factory
        self.components = components
        self.elementName = factory.name
    }
}

// MARK: - View-block components (also legal at module-level for function-only modules)

/// Type-erased setter the framework calls on prop dispatch.
/// `view` is the native element instance the value was set on; `value`
/// is the raw `WhiskerValue` — no auto-deserialization, the author
/// destructures it, e.g. `value.asString()`.
public typealias WhiskerPropSetter = (_ view: AnyObject, _ value: WhiskerValue) -> Void
public typealias WhiskerPropClearer = (_ view: AnyObject) -> Void

/// Prop component — a single named setter on the view class.
public struct WhiskerPropComponent: WhiskerDefinitionComponent {
    public let name: String
    public let setter: WhiskerPropSetter
    public let clearer: WhiskerPropClearer
    public init(
        name: String,
        clearer: @escaping WhiskerPropClearer = { _ in },
        setter: @escaping WhiskerPropSetter
    ) {
        self.name = name
        self.clearer = clearer
        self.setter = setter
    }
}

/// Type-erased function handler. `args` are the raw positional
/// `WhiskerValue`s from the Rust call site — no auto-deserialization,
/// the author destructures, e.g. `args[0].asDouble()`; the result is
/// a raw `WhiskerValue` (`.null` for "no result"). `view` is `nil` for module-level
/// `Function`s, the native element instance for view-block `Function`s.
public typealias WhiskerFunctionHandler = (_ view: AnyObject?, _ args: [WhiskerValue]) -> WhiskerValue

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

/// Type-erased ASYNC function handler. Like `WhiskerFunctionHandler`
/// but instead of returning a value it resolves the given `promise`
/// — now or later, e.g. from a StoreKit / URLSession completion.
/// `view` is `nil` for module-level `AsyncFunction`s. Mirrors Expo's
/// `AsyncFunction` + `Promise` (the callback-resolved form).
public typealias WhiskerAsyncFunctionHandler =
    (_ view: AnyObject?, _ args: [WhiskerValue], _ promise: WhiskerPromise) -> Void

/// Async function component — the async parallel of
/// `WhiskerFunctionComponent`. The Host hands the handler a
/// `WhiskerPromise` wrapping the FFI completion callback.
public struct WhiskerAsyncFunctionComponent: WhiskerDefinitionComponent {
    public let name: String
    public let handler: WhiskerAsyncFunctionHandler
    public init(name: String, handler: @escaping WhiskerAsyncFunctionHandler) {
        self.name = name
        self.handler = handler
    }
}

/// One-way command applied to a mounted Host View.
public typealias WhiskerCommandHandler = (_ view: AnyObject, _ parameters: WhiskerValue) -> Void

public struct WhiskerCommandComponent: WhiskerDefinitionComponent {
    public let name: String
    public let handler: WhiskerCommandHandler
    public init(name: String, handler: @escaping WhiskerCommandHandler) {
        self.name = name
        self.handler = handler
    }
}

/// Resolved inherited text-style consumer for a native View.
public typealias WhiskerTextStyleHandler = (_ view: AnyObject, _ style: WhiskerTextStyle) -> Void

public struct WhiskerTextStyleComponent: WhiskerDefinitionComponent {
    public let handler: WhiskerTextStyleHandler
    public init(handler: @escaping WhiskerTextStyleHandler) {
        self.handler = handler
    }
}

/// Synchronous intrinsic measurement provider for one View declaration.
public struct WhiskerMeasurementComponent: WhiskerDefinitionComponent {
    public let handler: (WhiskerMeasureRequest) -> WhiskerMeasuredSize?
    public init(handler: @escaping (WhiskerMeasureRequest) -> WhiskerMeasuredSize?) {
        self.handler = handler
    }
}

/// Event-name declaration component. Metadata only; dispatch from
/// inside the module body goes through
/// `Module.sendEvent(name, payload)` — the bridge fans the payload
/// out to every Rust subscriber registered via
/// `PlatformModule::on_event`.
///
/// The matching `OnStartObserving` / `OnStopObserving` blocks below
/// let the module lazily attach / detach its underlying source.
public struct WhiskerEventsComponent: WhiskerDefinitionComponent {
    public let names: [String]
    public init(_ names: [String]) {
        self.names = names
    }
}

/// `OnStartObserving("name") { ... }` — fires when the listener
/// count for `(this module, "name")` transitions from 0 to 1. The
/// closure typically attaches the platform source (e.g. registers
/// an `OnBackInvokedCallback`, opens a sensor) so the work only
/// runs while subscribers are active.
public struct WhiskerOnStartObservingComponent: WhiskerDefinitionComponent {
    public let eventName: String
    public let handler: () -> Void
    public init(eventName: String, handler: @escaping () -> Void) {
        self.eventName = eventName
        self.handler = handler
    }
}

/// `OnStopObserving("name") { ... }` — fires when the listener
/// count for `(this module, "name")` transitions from 1 to 0. The
/// closure tears down whatever `OnStartObserving` set up.
public struct WhiskerOnStopObservingComponent: WhiskerDefinitionComponent {
    public let eventName: String
    public let handler: () -> Void
    public init(eventName: String, handler: @escaping () -> Void) {
        self.eventName = eventName
        self.handler = handler
    }
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

/// The assembled definition the framework registers with the active Host at
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

    /// Explicit module name. View identity is never a fallback.
    public var name: String? {
        for c in components {
            if let n = c as? WhiskerNameComponent { return n.value }
        }
        return nil
    }

    /// First View block, retained for source compatibility.
    public var view: WhiskerViewComponent? {
        views.first
    }

    /// Every Host View contributed by this module.
    var views: [WhiskerViewComponent] {
        components.compactMap { $0 as? WhiskerViewComponent }
    }

    /// Module-level (view-less) functions — i.e. `Function(...)`
    /// declared OUTSIDE a `View(...)` block.
    public var functions: [WhiskerFunctionComponent] {
        components.compactMap { $0 as? WhiskerFunctionComponent }
    }

    /// Module-level (view-less) async functions — `AsyncFunction(...)`
    /// declared OUTSIDE a `View(...)` block.
    public var asyncFunctions: [WhiskerAsyncFunctionComponent] {
        components.compactMap { $0 as? WhiskerAsyncFunctionComponent }
    }

    /// Module-scoped event declarations. View events remain inside their View block.
    public var events: [String] {
        components.compactMap { $0 as? WhiskerEventsComponent }.flatMap(\.names)
    }

    /// All `OnStartObserving(...)` hooks declared at module-level.
    public var onStartObservingHooks: [WhiskerOnStartObservingComponent] {
        components.compactMap { $0 as? WhiskerOnStartObservingComponent }
    }

    /// All `OnStopObserving(...)` hooks declared at module-level.
    public var onStopObservingHooks: [WhiskerOnStopObservingComponent] {
        components.compactMap { $0 as? WhiskerOnStopObservingComponent }
    }

    /// Validate declaration-local invariants before runtime negotiation.
    public func validateElementDeclaration() {
        let names = components.compactMap { $0 as? WhiskerNameComponent }
        precondition(
            names.count == 1 && !names[0].value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
            "ModuleDefinition requires exactly one non-empty Name"
        )
        let functionNames = functions.map(\.name) + asyncFunctions.map(\.name)
        precondition(
            functionNames.allSatisfy { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                && Set(functionNames).count == functionNames.count,
            "module Function and AsyncFunction names must be non-empty and unique"
        )
        precondition(
            events.allSatisfy { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                && Set(events).count == events.count,
            "module Event names must be non-empty and unique"
        )
        let declaredEvents = Set(events)
        let startEvents = onStartObservingHooks.map(\.eventName)
        let stopEvents = onStopObservingHooks.map(\.eventName)
        precondition(Set(startEvents).count == startEvents.count, "duplicate OnStartObserving hook")
        precondition(Set(stopEvents).count == stopEvents.count, "duplicate OnStopObserving hook")
        precondition(
            (startEvents + stopEvents).allSatisfy(declaredEvents.contains),
            "observer hooks must reference a module Event declaration"
        )
        for view in views {
            precondition(
                view.elementName?.isEmpty == false,
                "every View requires an explicit package-qualified element name"
            )
            let props = view.components.compactMap { ($0 as? WhiskerPropComponent)?.name }
            precondition(Set(props).count == props.count, "duplicate Host property")
            let events = view.components.compactMap { $0 as? WhiskerEventsComponent }.flatMap(\.names)
            precondition(
                events.count <= UInt64.bitWidth
                    && events.allSatisfy { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
                    && Set(events).count == events.count,
                "View Events must contain at most 64 non-empty unique names"
            )
            let commands = view.components.compactMap { ($0 as? WhiskerCommandComponent)?.name }
            precondition(Set(commands).count == commands.count, "duplicate Host command")
            precondition(
                view.components.compactMap { $0 as? WhiskerTextStyleComponent }.count <= 1,
                "duplicate TextStyle consumer"
            )
            precondition(
                view.components.compactMap { $0 as? WhiskerMeasurementComponent }.count <= 1,
                "duplicate Measurement provider"
            )
        }
    }
}

// MARK: - Top-level factories — the DSL surface authors call

// Naming convention: PascalCase to mirror Expo Modules. Swift
// allows top-level function names of any case.

/// `Name("Foo")` — the module's local tag name.
public func Name(_ value: String) -> WhiskerDefinitionComponent {
    WhiskerNameComponent(value)
}

/// `View(MyView.self) { ... }` — registers a native View class +
/// its inner DSL block (Prop / Command / Events).
public func View<V: AnyObject>(
    _ viewClass: V.Type,
    @WhiskerViewDefinitionBuilder _ body: () -> [WhiskerDefinitionComponent]
) -> WhiskerDefinitionComponent {
    WhiskerViewComponent(viewClass: viewClass, components: body())
}

/// Registers a Host factory through the same multi-View module DSL.
public func View(_ factory: WhiskerElementFactory) -> WhiskerDefinitionComponent {
    WhiskerViewComponent(factory: factory, components: [])
}

/// Registers a Host factory with the same Prop / Command / Events block used
/// by class-backed views.
public func View(
    _ factory: WhiskerElementFactory,
    @WhiskerViewDefinitionBuilder _ body: () -> [WhiskerDefinitionComponent]
) -> WhiskerDefinitionComponent {
    WhiskerViewComponent(factory: factory, components: body())
}

/// String-declared element identity, resolved to Rust IDs at bootstrap.
public func View<V: AnyObject>(
    _ elementName: String,
    _ viewClass: V.Type,
    @WhiskerViewDefinitionBuilder _ body: () -> [WhiskerDefinitionComponent]
) -> WhiskerDefinitionComponent {
    WhiskerViewComponent(
        viewClass: viewClass,
        components: body(),
        elementName: elementName
    )
}

/// `Events("a", "b", ...)` — declare event names this module
/// emits. Metadata only; dispatch goes through
/// `Module.sendEvent(name, payload)`.
public func Events(_ names: String...) -> WhiskerDefinitionComponent {
    WhiskerEventsComponent(names)
}

/// `OnStartObserving("name") { ... }` — declare a lazy-start hook
/// for `name`. The closure fires once on the 0→1 listener-count
/// transition. Use for sources that are expensive to keep open
/// (`OnBackInvokedCallback`, motion sensors, network sockets).
public func OnStartObserving(
    _ name: String,
    _ handler: @escaping () -> Void
) -> WhiskerDefinitionComponent {
    WhiskerOnStartObservingComponent(eventName: name, handler: handler)
}

/// `OnStopObserving("name") { ... }` — pair to `OnStartObserving`.
/// The closure fires once on the 1→0 listener-count transition.
public func OnStopObserving(
    _ name: String,
    _ handler: @escaping () -> Void
) -> WhiskerDefinitionComponent {
    WhiskerOnStopObservingComponent(eventName: name, handler: handler)
}

// MARK: - Prop factories

/// `Prop("src") { (view: VideoView, value) in view.setSrc(value.asString()) }`
/// — view-bearing prop setter. `value` is the raw
/// `WhiskerValue`; the author destructures it (`.asString()`,
/// `.asDouble()`, …). The framework downcasts the view and
/// silently no-ops on a view-type mismatch (debug-build log).
public func Prop<V: AnyObject>(
    _ name: String,
    clear: @escaping (V) -> Void = { _ in },
    _ setter: @escaping (V, WhiskerValue) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerPropComponent(
        name: name,
        clearer: { uiAny in if let ui = uiAny as? V { clear(ui) } }
    ) { uiAny, value in
        guard let ui = uiAny as? V else {
            #if DEBUG
            print("WhiskerModule: Prop(\"\(name)\") view type mismatch — expected \(V.self), got \(type(of: uiAny))")
            #endif
            return
        }
        setter(ui, value)
    }
}

/// Receives the resolved inherited text style for a native View.
public func TextStyle<V: AnyObject>(
    _ handler: @escaping (V, WhiskerTextStyle) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerTextStyleComponent { view, style in
        if let typed = view as? V { handler(typed, style) }
    }
}

/// Supplies Host intrinsic metrics for Custom/ReplacedContent schemas.
public func Measurement(
    _ handler: @escaping (WhiskerMeasureRequest) -> WhiskerMeasuredSize?
) -> WhiskerDefinitionComponent {
    WhiskerMeasurementComponent(handler: handler)
}

// MARK: - Function factories (module-level raw `[WhiskerValue]`)

// Two forms: view-bound (inside a `View(...)` block; closure gets
// the view + raw args) and module-level (function-only module; raw
// args only). Both pass the raw `[WhiskerValue]` through unchanged
// — no arity overloads, no type coercion. The closure returns a
// `WhiskerValue` (`.null` for "no result").

/// One-way command on a mounted View.
public func Command<V: AnyObject>(
    _ name: String,
    _ handler: @escaping (V, WhiskerValue) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerCommandComponent(name: name) { viewAny, parameters in
        guard let view = viewAny as? V else { return }
        handler(view, parameters)
    }
}

@available(*, deprecated, message: "View Function is not part of whisker-module v1; use Command")
public func Function<V: AnyObject>(
    _ name: String,
    _ handler: @escaping (V, [WhiskerValue]) -> WhiskerValue
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { viewAny, args in
        guard let view = viewAny as? V else {
            return .error("Function(\"\(name)\") view type mismatch")
        }
        return handler(view, args)
    }
}

/// `Function("save") { args in .bool(...) }` — module-level
/// (view-less) sync function for a function-only module.
public func Function(
    _ name: String,
    _ handler: @escaping ([WhiskerValue]) -> WhiskerValue
) -> WhiskerDefinitionComponent {
    WhiskerFunctionComponent(name: name) { _, args in handler(args) }
}

// MARK: - AsyncFunction factories (Expo-style `AsyncFunction` + `Promise`)

@available(*, deprecated, message: "View AsyncFunction is not part of whisker-module v1")
public func AsyncFunction<V: AnyObject>(
    _ name: String,
    _ handler: @escaping (V, [WhiskerValue], WhiskerPromise) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerAsyncFunctionComponent(name: name) { viewAny, args, promise in
        guard let view = viewAny as? V else {
            return promise.reject("AsyncFunction(\"\(name)\") view type mismatch")
        }
        handler(view, args, promise)
    }
}

/// `AsyncFunction("getOfferings") { (args, promise) in ... }` —
/// module-level (view-less) async function for a function-only module.
public func AsyncFunction(
    _ name: String,
    _ handler: @escaping ([WhiskerValue], WhiskerPromise) -> Void
) -> WhiskerDefinitionComponent {
    WhiskerAsyncFunctionComponent(name: name) { _, args, promise in handler(args, promise) }
}
