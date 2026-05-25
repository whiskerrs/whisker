// `Module` base class (iOS) — the API a Whisker module subclasses.
// Mark the subclass with `@WhiskerModule` (from `WhiskerComponents`)
// so the SwiftPM codegen plugin discovers it and generates the Lynx
// registration. (Modeled after Expo's `Module` base class; the
// `@WhiskerModule` marker plays the role of Expo's
// `expo-module.config.json` entry — but inline at the declaration.)
//
// ```swift
// import WhiskerComponents   // @WhiskerModule
// import WhiskerModuleApi    // Module, ModuleDefinition, DSL
//
// @WhiskerModule
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
