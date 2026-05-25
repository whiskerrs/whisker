// Phase L-2a — `WhiskerModule` base class (iOS).
//
// Authors subclass `WhiskerModule` and override `definition()` to
// declare their module via the result-builder DSL in
// `ModuleDefinition.swift`. The DSL surface is fully usable
// (compiles, type-checks, runs) as of L-2a; the dispatch wiring
// that translates `definition()` into Lynx prop / method
// registrations lands in Phase L-2b.
//
// The existing `@WhiskerComponent` / `@WhiskerProp` /
// `@WhiskerUIMethod` annotation surface continues to drive real
// registrations in parallel — both paths coexist through Phase
// L-3, and the annotation surface gets deprecated in Phase M.

import Foundation

/// Base class for Whisker modules.
///
/// ```swift
/// public final class VideoModule: WhiskerModule {
///     public override func definition() -> ModuleDefinition {
///         Name("Video")
///         View(WhiskerVideoView.self) {
///             Prop("src") { (view: WhiskerVideoView, value: String) in view.setSrc(value) }
///             Function("play") { (view: WhiskerVideoView) in view.play() }
///         }
///     }
/// }
/// ```
///
/// Phase L-2a: the base class collects the definition lazily on
/// first access (`self.definitionLazy`) and exposes it for
/// inspection but does not yet wire it into Lynx. Phase L-2b's
/// registration entry point reads `definitionLazy` at app launch
/// and installs the prop / method dispatch.
open class WhiskerModule {
    /// Designated init — empty so subclasses don't have to forward
    /// arguments. State setup happens inside `definition()` (or
    /// inside lifecycle hooks added in a follow-up).
    ///
    /// `required` so the codegen-emitted registration block can
    /// construct an instance from a metatype value
    /// (`cls.init()` where `cls: WhiskerModule.Type` is resolved
    /// via `NSClassFromString`). Without `required`, Swift rejects
    /// `metatype.init()` at compile time.
    public required init() {}

    /// Authors override to declare the module via the DSL.
    /// Default impl returns an empty definition — useful for tests
    /// and as a sentinel during the L-2a→L-2b migration.
    open func definition() -> ModuleDefinition {
        ModuleDefinition(components: [])
    }

    /// Cached `definition()` value. Computed on first access on
    /// the main thread (module registration runs at app-launch
    /// time before any background work) and re-used afterwards
    /// so authors can declare expensive setup in `definition()`
    /// without paying for it on every dispatch.
    public private(set) lazy var definitionLazy: ModuleDefinition = self.definition()
}
