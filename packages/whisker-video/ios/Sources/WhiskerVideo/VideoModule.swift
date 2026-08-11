// `whisker-video` ModuleDefinition (iOS).
//
// The `VideoView` Lynx UI subclass this references lives in
// `VideoView.swift`. Same split on Android (`VideoModule.kt` +
// `VideoView.kt`).

import WhiskerModule    // Module, ModuleDefinition, DSL

/// DSL-driven module. Subclassing `Module` is the registration signal:
/// the SwiftPM codegen plugin scans the target's sources for concrete
/// `Module` subclasses and emits a registration block in
/// `<Target>+Generated.swift` that registers
/// `definitionLazy.view!.viewClass` with `LynxComponentRegistry` under
/// "whisker-video:Video", then calls `module.registerWithLynx()` so the
/// props and functions install via the Obj-C-runtime path.
public final class VideoModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Video")
            View(VideoView.self) {
                Prop("src") { (view: VideoView, value: WhiskerValue) in
                    view.setSrc(value.asString ?? "")
                }
                Function("play")  { (view: VideoView, _: [WhiskerValue]) in view.play();  return .null }
                Function("pause") { (view: VideoView, _: [WhiskerValue]) in view.pause(); return .null }
                Function("seek")  { (view: VideoView, args: [WhiskerValue]) in
                    view.seek(args.first?.asDouble ?? 0)
                    return .null
                }
            }
        }
    }
}
