// `whisker-svg` ModuleDefinition (iOS).
//
// Mirrors `whisker-image` / `whisker-safe-area` — the codegen
// plugin scans this Swift target for any concrete `Module`
// subclass and emits a registration block in
// `<Target>+Generated.swift` that:
//
//   * Reads the declared `WhiskerSvgView` class.
//   * Registers it as `"whisker-svg:Svg"` with Whisker's Host registry.
//   * Installs the `_display_list` and `color` prop handlers.
//
// User code never instantiates this directly — the Rust crate's
// `Svg(content, color, style)` component compiles the SVG and
// renders an internal `SvgRenderer` element bound to this view.

import WhiskerModule

@WhiskerModule
public final class SvgModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("Svg")
            View("whisker.svg/Svg", WhiskerSvgView.self) {
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
