// Whisker module element hosting an ImageView with Coil-driven URL loading.

package rs.whisker.elements.image

import android.content.Context
import android.widget.ImageView
import coil.dispose
import coil.load
import coil.request.CachePolicy
import coil.request.Disposable
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI

open class WhiskerImageView(context: WhiskerContext) : WhiskerUI<ImageView>(context) {

    private var currentSrc: String? = null
    private var currentHeaders: Map<String, String> = emptyMap()
    private var currentRequest: Disposable? = null
    override fun createView(context: Context): ImageView {
        val v = ImageView(context)
        // `CENTER_CROP` matches the module's `aspectFill` default.
        v.scaleType = ImageView.ScaleType.CENTER_CROP
        return v
    }

    /**
     * Backing of the `src` prop. Kicks off a Coil request bound to the
     * ImageView; the returned Disposable is cancelled before the next
     * request is issued, so a second `setSrc` cancels the in-flight one.
     */
    fun setSrc(value: String) {
        // Coil would short-circuit an unchanged src through its memory
        // cache anyway, but constructing the request still costs something
        // on every benign re-render.
        if (currentSrc == value) return
        currentSrc = value
        reload()
    }

    /// Backing of the `headers` prop: a JSON object of request headers.
    /// Re-fetches, because a host that answers differently per header
    /// answers differently per change.
    fun setHeaders(json: String) {
        val parsed = parseHeaders(json)
        if (parsed == currentHeaders) return
        currentHeaders = parsed
        reload()
    }

    private fun parseHeaders(json: String): Map<String, String> {
        if (json.isBlank()) return emptyMap()
        return runCatching {
            val object_ = org.json.JSONObject(json)
            object_.keys().asSequence().associateWith { object_.optString(it) }
        }.getOrDefault(emptyMap())
    }

    /// Backing of the `mode` prop, mapping module values to `ImageView.ScaleType`.
    fun setMode(value: String) {
        val imageView = view()
        imageView.scaleType = when (value) {
            "aspectFill" -> ImageView.ScaleType.CENTER_CROP
            "aspectFit" -> ImageView.ScaleType.FIT_CENTER
            "scaleToFill" -> ImageView.ScaleType.FIT_XY
            "center" -> ImageView.ScaleType.CENTER
            else -> ImageView.ScaleType.CENTER_CROP
        }
    }

    /**
     * Issue (or re-issue) a Coil request for the current `src` with the
     * current request options.
     */
    private fun reload() {
        val src = currentSrc ?: return
        val imageView = view()

        // `dispose()` is a no-op on an already-completed disposable.
        currentRequest?.dispose()
        imageView.dispose()

        if (src.isBlank()) {
            imageView.setImageDrawable(null)
            currentRequest = null
            return
        }

        currentRequest = imageView.load(src) {
            crossfade(200)
            // Two requests that differ only by header are two different
            // resources: the cache is keyed by URL alone, so without
            // this a header change hands back the answer to the old one.
            if (currentHeaders.isNotEmpty()) {
                val key = src + "|" + currentHeaders.entries
                    .sortedBy { it.key }
                    .joinToString(";") { "${it.key}=${it.value}" }
                memoryCacheKey(key)
                diskCacheKey(key)
            }
            // The outcome is reported either way: a page that 403s is
            // otherwise a blank the app never hears about.
            listener(
                onSuccess = { _, result ->
                    WhiskerCustomEvent.dispatch(
                        this@WhiskerImageView,
                        "load",
                        mapOf(
                            "width" to result.drawable.intrinsicWidth,
                            "height" to result.drawable.intrinsicHeight,
                        ),
                    )
                },
                onError = { _, result ->
                    WhiskerCustomEvent.dispatch(
                        this@WhiskerImageView,
                        "error",
                        mapOf("error" to (result.throwable.message ?: "load failed")),
                    )
                },
            )
            // Hot-link protection is the reason: those hosts answer 403
            // unless the request carries the `Referer` their own pages
            // send.
            for ((name, value) in currentHeaders) {
                addHeader(name, value)
            }
            memoryCachePolicy(CachePolicy.ENABLED)
            diskCachePolicy(CachePolicy.ENABLED)
        }
    }
}
