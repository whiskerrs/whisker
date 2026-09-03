// `whisker-video` ModuleDefinition (iOS).
//
// The `VideoView` Whisker module view this references lives in
// `VideoView.swift`. Same split on Android (`VideoModule.kt` +
// `VideoView.kt`).

import WhiskerModule    // Module, ModuleDefinition, DSL

/// DSL-driven module. `@WhiskerModule` is the registration signal:
/// the SwiftPM codegen plugin scans the target's sources for annotated
/// module declarations and emits a registration block in
/// `<Target>+Generated.swift`, which registers the view and module functions
/// with Whisker's Host registry.
@WhiskerModule
public final class VideoModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Video")
            View("whisker-video:Video", VideoView.self) {
                Prop("src", clear: { (view: VideoView) in view.setSrc("") }) { (view: VideoView, value: WhiskerValue) in
                    view.setSrc(value.asString ?? "")
                }
                Command("play")  { (view: VideoView, _: WhiskerValue) in view.play() }
                Command("pause") { (view: VideoView, _: WhiskerValue) in view.pause() }
                Command("seek")  { (view: VideoView, parameters: WhiskerValue) in
                    view.seek(parameters.asDouble ?? 0)
                }
            }
        }
    }
}
