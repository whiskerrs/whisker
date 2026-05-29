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

import WhiskerModule    // Module, ModuleDefinition, DSL

/// DSL-driven module. Subclassing `Module` is the registration
/// signal — the SwiftPM codegen plugin (L-3) scans the target's
/// sources for any concrete subclass of `Module` and emits a
/// registration block in `<Target>+Generated.swift` that:
///
///   - Reads `definitionLazy.view!.viewClass` (== `VideoView`).
///   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
///     "whisker-video:Video")`.
///   - Calls `module.registerWithLynx()` so the DSL's `Prop("src")`
///     + `Function("play"/"pause"/"seek")` install via the
///     Obj-C-runtime path (L-2b).
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
