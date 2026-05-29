// Phase 7-Φ.H.1: Lynx symbol hiding (iOS).
//
// A view-bearing Whisker module's `View(...)` block references a
// Lynx UI subclass, which previously had to import Lynx types
// directly:
//
// ```swift
// import Lynx
//
// public final class HelloView: LynxUI<UIView> {
//     @objc public override func createView() -> UIView { … }
// }
// ```
//
// The bridge runtime is built on Lynx and that won't change in
// the foreseeable future, but the Lynx-ness leaking into every
// module's public API surface makes Whisker feel like a thin
// Lynx wrapper rather than its own framework. These typealiases
// give module authors `Whisker*` symbols that resolve to their
// Lynx counterparts at the Swift type-system level — same
// runtime classes, just a presentation rename.
//
// ```swift
// import WhiskerModule
//
// public final class HelloView: WhiskerUI<UIView> {
//     @objc public override func createView() -> UIView { … }
// }
// ```
//
// Stack traces / debugger views still surface the real `LynxUI`
// class names (typealiases are purely a source-level concept).
// Renaming the underlying classes themselves would require
// patching the Lynx fork — a separate, larger effort planned for
// the long-term roadmap.

// `@_exported` so module-author `.swift` files can `import
// WhiskerRuntime` alone — the typealias targets (`LynxUI`,
// `LynxContext`, …) get pulled into scope transitively, no
// separate `import Lynx` required. Without this the typealiases
// resolve at WhiskerRuntime's own compile but fail at the
// consumer's call site with "cannot find type 'LynxUI' in scope".
@_exported import Lynx

/// Whisker's preferred alias for Lynx's `LynxUI` generic base.
/// Subclass this for the view class a module's `View(...)` block
/// references.
///
/// The generic parameter is the native view type the element
/// wraps (`UIView` / `UITextField` / …). Lynx's create / layout
/// / event hooks all surface through this base unchanged.
public typealias WhiskerUI = LynxUI

/// Per-`WhiskerView` context, identical to Lynx's `LynxContext`.
/// Module code typically only needs it for
/// `lynxContext.eventEmitter.dispatchCustomEvent(...)` (see
/// [`WhiskerCustomEvent`] for the high-level wrapper).
public typealias WhiskerContext = LynxContext

/// Custom-event payload type for `WhiskerUI` subclasses that
/// emit events the host app's `on_<event>:` Rust callbacks
/// receive. Use [`WhiskerCustomEvent`] for the dispatch helper.
public typealias WhiskerCustomEventBase = LynxCustomEvent

/// Lynx's component-registration entry point. Module authors
/// rarely call this directly — the `WhiskerModuleCodegenPlugin`
/// generates the registration code from each discovered `Module`
/// subclass's `View(...)` block.
public typealias WhiskerComponentRegistry = LynxComponentRegistry

// MARK: - Custom-event dispatch helper

/// Whisker-branded façade over Lynx's `LynxCustomEvent` +
/// `LynxUIContext.eventEmitter.dispatchCustomEvent(_:)`.
///
/// Module authors that need to push events back to Rust (e.g. an
/// `Input` element's text-change firing `on_input:` on the consumer
/// crate) call:
///
/// ```swift
/// WhiskerCustomEvent.dispatch(
///     from: self,                       // the WhiskerUI subclass
///     name: "input",
///     params: ["value": textField.text ?? ""])
/// ```
///
/// instead of manually constructing `LynxCustomEvent` and
/// reaching into `context.eventEmitter`. The function looks at
/// the UI's `sign` + `context` to wire the event back to the
/// host's `whisker_bridge_set_event_listener_with_payload`
/// reporter, which delivers the JSON-serialised params to the
/// matching Rust `on_<event>: String` callback.
public enum WhiskerCustomEvent {
    /// Build and dispatch a `LynxCustomEvent` from `ui`.
    /// No-op if the UI has been detached from its context
    /// (Lynx holds `context` weakly).
    public static func dispatch<V>(
        from ui: WhiskerUI<V>,
        name: String,
        params: [AnyHashable: Any]? = nil
    ) {
        guard let ctx = ui.context else { return }
        let event = LynxCustomEvent(
            name: name,
            targetSign: ui.sign,
            params: params
        )
        // `eventEmitter` is `nullable` in LynxUIContext.h, so the
        // imported Swift type is `LynxEventEmitter?`. Skip dispatch
        // when the emitter hasn't been wired yet (pre-mount or
        // post-teardown).
        ctx.eventEmitter?.dispatchCustomEvent(event)
    }
}
