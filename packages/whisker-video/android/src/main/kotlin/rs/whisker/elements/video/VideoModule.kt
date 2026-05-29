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

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

/**
 * DSL-driven module. Subclassing [Module] is the registration
 * signal — the KSP processor finds every concrete subclass and
 * emits the registration block into
 * `WhiskerVideoBehaviors.registerAll()`. This module declares:
 *   - Tag name `Video` (registers as `whisker-video:Video`).
 *   - View class [VideoView].
 *   - One prop setter (`src`).
 *   - Three sync method dispatchers (`play`, `pause`, `seek`).
 */
class VideoModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Video")
        View(VideoView::class.java) {
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
