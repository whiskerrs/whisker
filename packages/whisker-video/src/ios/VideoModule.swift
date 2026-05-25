// Phase L-3 — `whisker-video` ModuleDefinition (iOS).
//
// Replaces the pre-L-3 `@WhiskerComponent("Video")`-annotated
// `WhiskerVideoComponent`. The Lynx tag stays `whisker-video:Video`;
// the DSL's `Prop("src")` / `Function("play"/"pause"/"seek")`
// expand into the same Lynx-visible setters / methods via the
// SwiftPM codegen plugin + Obj-C-runtime install (L-2b).
//
// The `VideoView` Lynx UI subclass this references lives in
// `VideoView.swift`. Same split on Android (`VideoModule.kt` +
// `VideoView.kt`).

import WhiskerModuleApi

/// DSL-driven module. The SwiftPM codegen plugin (L-3) discovers
/// this class by spotting the `: WhiskerModule` inheritance, then
/// emits a registration block in `<Target>+Generated.swift` that:
///
///   - Reads `definitionLazy.view!.viewClass` (== `VideoView`).
///   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
///     "whisker-video:Video")`.
///   - Calls `module.registerWithLynx()` so the DSL's `Prop("src")`
///     + `Function("play"/"pause"/"seek")` install via the
///     Obj-C-runtime path (L-2b).
public final class VideoModule: WhiskerModule {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Video")
            View(VideoView.self) {
                Prop("src") { (view: VideoView, value: String) in
                    view.setSrc(value)
                }
                Function("play")  { (view: VideoView) in view.play()  }
                Function("pause") { (view: VideoView) in view.pause() }
                Function("seek")  { (view: VideoView, seconds: Double) in
                    view.seek(seconds)
                }
            }
        }
    }
}
