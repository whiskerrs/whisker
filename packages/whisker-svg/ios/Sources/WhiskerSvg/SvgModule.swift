// `whisker-svg` ModuleDefinition (iOS).
//
// Mirrors `whisker-image` / `whisker-safe-area` — the codegen
// plugin scans this Swift target for any concrete `Module`
// subclass and emits a registration block in
// `<Target>+Generated.swift` that:
//
//   * Reads `definitionLazy.view!.viewClass` (== WhiskerSvgView).
//   * Calls `LynxComponentRegistry.registerUI(viewClass, withName:
//     "whisker-svg:Svg")`.
//   * Calls `module.registerWithLynx()` so the DSL's
//     `Prop("_display_list")` / `Prop("color")` install via the
//     Obj-C-runtime path the rest of Whisker uses.
//
// User code never instantiates this directly — the Rust crate's
// `Svg(content, color, style)` component compiles the SVG and
// renders an internal `SvgRenderer` element bound to this view.

import WhiskerModule

public final class SvgModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Svg")
            View(WhiskerSvgView.self) {
                Prop("display-list") { (view: WhiskerSvgView, value: WhiskerValue) in
                    view.setDisplayList(value.asString ?? "")
                }
                Prop("color") { (view: WhiskerSvgView, value: WhiskerValue) in
                    view.setColor(value.asString ?? "")
                }
            }
        }
    }
}
