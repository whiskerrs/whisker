// `Module` base class (iOS) — the API a Whisker module subclasses.
// **Subclassing is the registration signal** — the SwiftPM codegen
// plugin (`WhiskerModuleCodegen`) walks every concrete subclass of
// `Module` and emits the Lynx registration. No marker attribute is
// required at the declaration site.
//
// ```swift
// import WhiskerModule    // Module, ModuleDefinition, DSL
//
// public final class VideoModule: Module {
//     public override func definition() -> ModuleDefinition {
//         Name("Video")
//         View(VideoView.self) {
//             Prop("src") { (view: VideoView, value: String) in view.setSrc(value) }
//             Function("play") { (view: VideoView) in view.play() }
//         }
//     }
// }
// ```

import Foundation

open class Module {
    /// Designated init — empty so subclasses don't have to forward
    /// arguments. State setup happens inside `definition()`.
    public init() {}

    /// Authors override to declare the module via the DSL. Default
    /// impl returns an empty definition — useful for tests and as a
    /// sentinel.
    open func definition() -> ModuleDefinition {
        ModuleDefinition(components: [])
    }

    /// Cached `definition()` value — computed on first access (module
    /// registration runs at app-launch time, single-threaded) and
    /// re-used afterwards so authors can do expensive setup in
    /// `definition()` without paying for it on every dispatch.
    public private(set) lazy var definitionLazy: ModuleDefinition = self.definition()
}
