// `whisker-image` ModuleDefinition (iOS).
//
// Mirrors `whisker-video`'s `VideoModule` shape — the codegen plugin
// scans this Swift target for any concrete subclass of `Module` and
// emits a registration block in `<Target>+Generated.swift` that:
//
//   - Reads `definitionLazy.view!.viewClass` (== `WhiskerImageView`).
//   - Calls `LynxComponentRegistry.registerUI(viewClass, withName:
//     "whisker-image:Image")`.
//   - Calls `module.registerWithLynx()` so the DSL's `Prop("src")` +
//     `Prop("mode")` install via the Obj-C-runtime path (L-2b).
//
// The `WhiskerImageView` Lynx UI subclass this references lives in
// `ImageView.swift`. Same split on Android (`ImageModule.kt` +
// `WhiskerImageView.kt`).

import WhiskerModule

public final class ImageModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Image")
            View(WhiskerImageView.self) {
                Prop("src") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setSrc(value.asString ?? "")
                }
                Prop("mode") { (view: WhiskerImageView, value: WhiskerValue) in
                    view.setMode(value.asString ?? "aspectFill")
                }
            }
        }
    }
}
