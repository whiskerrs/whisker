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
                Prop("src") { (view: FakeVideoView, value: WhiskerValue) in
                    view.setSrc(value.asString ?? "")
                }
                Function("play")  { (view: FakeVideoView, _: [WhiskerValue]) in view.play(); return .null }
                Function("pause") { (view: FakeVideoView, _: [WhiskerValue]) in view.pause(); return .null }
                Function("seek")  { (view: FakeVideoView, args: [WhiskerValue]) in
                    view.seek(args.first?.asDouble ?? 0)
                    return .null
                }
                Events("onCompleted")
            }
        }
    }

    // MARK: - Function-only (view-less) module

    internal static func localStoreModuleDefinition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerLocalStore")

            Function("save") { (args: [WhiskerValue]) -> WhiskerValue in
                let key = args.first?.asString ?? ""
                let value = args.count > 1 ? (args[1].asString ?? "") : ""
                return .bool(!key.isEmpty && !value.isEmpty)
            }
            Function("load") { (args: [WhiskerValue]) -> WhiskerValue in
                .string("stub-value-for-\(args.first?.asString ?? "")")
            }
        }
    }

    // MARK: - WhiskerModule subclass shape

    internal final class StubModule: Module {
        public override func definition() -> ModuleDefinition {
            ModuleDefinitionSamples.videoModuleDefinition()
        }
    }
}
