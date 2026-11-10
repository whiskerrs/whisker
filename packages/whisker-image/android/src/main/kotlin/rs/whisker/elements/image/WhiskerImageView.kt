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
    private var cacheOptions = ""
    private var requestVersion = 0L
    private var loadScheduled = false
    private val handler = android.os.Handler(android.os.Looper.getMainLooper())
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
        scheduleLoad()
    }

    /// Backing of the `headers` prop: a JSON object of request headers.
    /// Re-fetches, because a host that answers differently per header
    /// answers differently per change.
    fun setHeaders(json: String) {
        val parsed = parseHeaders(json)
        if (parsed == currentHeaders) return
        currentHeaders = parsed
        scheduleLoad()
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
    fun setCache(value: String) {
        if (cacheOptions == value) return
        cacheOptions = value
        scheduleLoad()
    }

    private fun scheduleLoad() {
        requestVersion++
        if (loadScheduled) return
        loadScheduled = true
        handler.post {
            loadScheduled = false
            reload()
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        handler.post {
            if (!isAttachedToWindow) {
                requestVersion++
                currentRequest?.dispose()
                view().dispose()
            }
        }
    }

    private fun reload() {
        val managed = if (cacheOptions.isNotEmpty()) runCatching { org.json.JSONObject(cacheOptions) }.getOrNull() else null
        val src = (if (cacheOptions.isEmpty()) currentSrc else managed?.optString("_src")) ?: return
        val imageView = view()

        // `dispose()` is a no-op on an already-completed disposable.
        currentRequest?.dispose()
        imageView.dispose()

        if (src.isBlank()) {
            imageView.setImageDrawable(null)
            currentRequest = null
            return
        }

        if (cacheOptions.isNotEmpty()) {
            imageView.setImageDrawable(null)
            val version = requestVersion
            val options = cacheOptions
            val headers = parseHeaders(managed?.optString("_headers").orEmpty())
            val context = imageView.context.applicationContext
            val weak = java.lang.ref.WeakReference(this)
            val completionHandler = handler
            ManagedImageCache.executor.execute {
                val result = runCatching { ManagedImageCache.load(context, src, headers, options) }
                completionHandler.post {
                    val owner = weak.get()
                    if (owner != null && owner.requestVersion == version) {
                        result.fold(onSuccess = { bitmap ->
                            owner.view().setImageBitmap(bitmap)
                            WhiskerCustomEvent.dispatch(owner, "load", mapOf("width" to bitmap.width, "height" to bitmap.height))
                        }, onFailure = { error ->
                            WhiskerCustomEvent.dispatch(owner, "error", mapOf("error" to (error.message ?: "image load failed")))
                        })
                    }
                }
            }
            return
        }
        currentRequest = imageView.load(src) {
            crossfade(200)
            // Two requests that differ only by header are two different
            // resources: the cache is keyed by URL alone, so without
            // this a header change hands back the answer to the old one.
            if (currentHeaders.isNotEmpty()) {
                val key = cacheKey(src, currentHeaders)
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

    companion object {
        /// URL plus the headers that shape the answer. Shared with
        /// `prefetch` so a prefetched image is found by the view that
        /// later shows it.
        fun cacheKey(src: String, headers: Map<String, String>): String =
            src + "|" + headers.entries
                .sortedBy { it.key }
                .joinToString(";") { "${it.key}=${it.value}" }
    }
}
