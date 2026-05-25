// Compile-time smoke checks for the L-2a `ModuleDefinition` DSL.
//
// These samples exist purely so a `swift build` failure against
// the DSL surface surfaces here at build time. Phase L-2a — no
// runtime behavior; just exercises the type surface end-to-end.

import Foundation

internal enum ModuleDefinitionSamples {

    // MARK: - View-bearing module

    internal class FakeVideoView {
        func setSrc(_ value: String) { /* noop */ }
        func play() { /* noop */ }
        func pause() { /* noop */ }
        func seek(_ seconds: Double) { /* noop */ }
    }

    internal static func videoModuleDefinition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Video")

            Constants(["maxResolution": "1080p"])

            View(FakeVideoView.self) {
                Prop("src") { (view: FakeVideoView, value: String) in
                    view.setSrc(value)
                }
                Function("play")  { (view: FakeVideoView) in view.play()  }
                Function("pause") { (view: FakeVideoView) in view.pause() }
                Function("seek")  { (view: FakeVideoView, seconds: Double) in
                    view.seek(seconds)
                }
                Events("onCompleted")
            }
        }
    }

    // MARK: - Function-only (view-less) module

    internal static func localStoreModuleDefinition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerLocalStore")

            Function("save") { (key: String, value: String) -> Bool in
                !key.isEmpty && !value.isEmpty
            }
            Function("load") { (key: String) -> String in
                "stub-value-for-\(key)"
            }
        }
    }

    // MARK: - WhiskerModule subclass shape

    internal final class StubModule: WhiskerModule {
        public override func definition() -> ModuleDefinition {
            ModuleDefinitionSamples.videoModuleDefinition()
        }
    }
}
