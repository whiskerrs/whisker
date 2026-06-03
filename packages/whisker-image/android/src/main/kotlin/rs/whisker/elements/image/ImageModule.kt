// `whisker-image` ModuleDefinition (Android).
//
// Mirrors `whisker-video`'s `VideoModule` shape — KSP scans this
// module's sources for any concrete `Module` subclass and emits the
// registration block into `WhiskerImageBehaviors.registerAll()`.
//
// The `WhiskerImageView` Lynx UI subclass this references lives in
// `WhiskerImageView.kt`. Same split on iOS (`ImageModule.swift` +
// `ImageView.swift`).

package rs.whisker.elements.image

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class ImageModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Image")
        View(WhiskerImageView::class.java) {
            Prop("src") { view: WhiskerImageView, value ->
                view.setSrc(value.asString() ?: "")
            }
            Prop("mode") { view: WhiskerImageView, value ->
                view.setMode(value.asString() ?: "aspectFill")
            }
        }
    }
}
