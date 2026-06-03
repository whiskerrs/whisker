// `whisker-svg` ModuleDefinition (Android).
//
// Mirrors `whisker-image` / `whisker-safe-area`'s shape: KSP scans
// this module's sources for any concrete `Module` subclass and emits
// the registration block into `WhiskerSvgBehaviors.registerAll()`.
//
// User code never instantiates this directly — the Rust crate's
// `Svg(content, color, style)` component compiles the SVG and
// renders an internal `SvgRenderer` element bound to this view.

package rs.whisker.modules.svg

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

class SvgModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Svg")
        View(WhiskerSvgView::class.java) {
            Prop("display-list") { view: WhiskerSvgView, value ->
                view.setDisplayList(value.asString() ?: "")
            }
            Prop("color") { view: WhiskerSvgView, value ->
                view.setColor(value.asString() ?: "")
            }
        }
    }
}
