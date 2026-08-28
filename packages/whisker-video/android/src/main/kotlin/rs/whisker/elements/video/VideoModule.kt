// `whisker-video` ModuleDefinition (Android).
//
// The `VideoView` Lynx UI subclass this references lives in
// `VideoView.kt`. Same split on iOS (`VideoModule.swift` +
// `VideoView.swift`).

package rs.whisker.elements.video

import rs.whisker.runtime.Module
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

/**
 * DSL-driven module. [WhiskerModule] is the registration signal:
 * the KSP processor finds every annotated module and emits the
 * registration block into `WhiskerVideoBehaviors.registerAll()`.
 */

@WhiskerModule
class VideoModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Video")
        View("whisker-video:Video", VideoView::class.java) {
            Prop("src") { view: VideoView, value ->
                view.setSrc(value.asString() ?: "")
            }
            Function("play") { view: VideoView, _ -> view.play(); WhiskerValue.Null }
            Function("pause") { view: VideoView, _ -> view.pause(); WhiskerValue.Null }
            Function("seek") { view: VideoView, args ->
                view.seek(args.getOrNull(0)?.asDouble() ?: 0.0)
                WhiskerValue.Null
            }
        }
    }
}
