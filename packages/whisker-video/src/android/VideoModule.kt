// Phase L-3 — `whisker-video` ModuleDefinition (Android).
//
// Replaces the pre-L-3 `@WhiskerComponent("Video")`-annotated
// `WhiskerVideoComponent`. The Lynx tag stays `whisker-video:Video`;
// the DSL's `Prop("src")` / `Function("play"/"pause"/"seek")` expand
// into the same Lynx-visible setters / invokers via KSP L-2c +
// the runtime install.
//
// The `VideoView` Lynx UI subclass this references lives in
// `VideoView.kt`. Same split on iOS (`VideoModule.swift` +
// `VideoView.swift`).

package rs.whisker.elements.video

import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule

/**
 * DSL-driven module. Subclasses [WhiskerModule] and declares:
 *   - Tag name `Video` (registers as `whisker-video:Video`).
 *   - View class [VideoView].
 *   - One prop setter (`src`).
 *   - Three sync method dispatchers (`play`, `pause`, `seek`).
 *
 * The KSP L-2c processor finds this class by superclass-chain
 * walk and emits the registration block into
 * `WhiskerVideoBehaviors.registerAll()`.
 */
class VideoModule : WhiskerModule() {
    override fun definition() = ModuleDefinition {
        Name("Video")
        View(VideoView::class.java) {
            Prop("src") { view: VideoView, value: String ->
                view.setSrc(value)
            }
            Function("play") { view: VideoView -> view.play() }
            Function("pause") { view: VideoView -> view.pause() }
            Function("seek") { view: VideoView, seconds: Double ->
                view.seek(seconds)
            }
        }
    }
}
