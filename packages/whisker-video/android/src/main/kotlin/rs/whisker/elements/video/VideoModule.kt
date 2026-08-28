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
            Command("play") { view: VideoView, _ -> view.play() }
            Command("pause") { view: VideoView, _ -> view.pause() }
            Command("seek") { view: VideoView, parameters ->
                view.seek(parameters.asDouble() ?: 0.0)
            }
        }
    }
}
