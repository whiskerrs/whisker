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
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue


@WhiskerModule
class ImageModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Image")

        // Prefetching belongs to no particular view — the pages after
        // the one on screen have no element yet.
        AsyncFunction("prefetch") { args: List<WhiskerValue>, promise ->
            val urls = (args.firstOrNull() as? WhiskerValue.Array)
                ?.value
                ?.mapNotNull { it.asString() }
                .orEmpty()
            val headers = args.getOrNull(1)?.asString().orEmpty()
            // The activity's application context: `AppContext` exposes
            // the live host, and Coil's loader belongs to the process
            // rather than to whatever is on screen.
            val context = appContext.currentActivity?.applicationContext
            if (context != null) {
                val parsed: kotlin.collections.Map<String, String> = runCatching {
                    val fields = org.json.JSONObject(headers)
                    fields.keys().asSequence().associateWith { fields.optString(it) }
                }.getOrDefault(emptyMap())
                val loader = coil.Coil.imageLoader(context)
                for (url in urls) {
                    val request = coil.request.ImageRequest.Builder(context)
                        .data(url)
                        .apply { parsed.forEach { (name, value) -> addHeader(name, value) } }
                        .build()
                    loader.enqueue(request)
                }
            }
            promise.resolve(WhiskerValue.Null)
        }
        View(WhiskerImageView::class.java) {
            Prop("src") { view: WhiskerImageView, value ->
                view.setSrc(value.asString() ?: "")
            }
            Prop("mode") { view: WhiskerImageView, value ->
                view.setMode(value.asString() ?: "aspectFill")
            }
            Prop("headers") { view: WhiskerImageView, value ->
                view.setHeaders(value.asString() ?: "")
            }
            Events("load", "error")
        }
    }
}
