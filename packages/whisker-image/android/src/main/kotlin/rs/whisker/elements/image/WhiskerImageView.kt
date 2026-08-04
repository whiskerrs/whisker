// Lynx UI subclass hosting an ImageView + Coil-driven URL loading.
// A plain `WhiskerUI` subclass — no Whisker annotations; registration
// is driven by `ImageModule`'s `definition()` (see `ImageModule.kt`).

package rs.whisker.elements.image

import android.content.Context
import android.widget.ImageView
import coil.dispose
import coil.load
import coil.request.CachePolicy
import coil.request.Disposable
import coil.transform.RoundedCornersTransformation
import com.lynx.tasm.behavior.StylesDiffMap
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI

open class WhiskerImageView(context: WhiskerContext) : WhiskerUI<ImageView>(context) {

    private var currentSrc: String? = null
    private var currentHeaders: Map<String, String> = emptyMap()
    private var currentRequest: Disposable? = null
    /// Active corner radius in **device pixels** — the value Lynx's
    /// CSS pipeline has already converted from the `8px` source. Fed
    /// directly to Coil's `RoundedCornersTransformation`, which also
    /// takes pixels. `0f` means "no rounding".
    private var cornerRadiusPx: Float = 0f

    override fun createView(context: Context): ImageView {
        val v = ImageView(context)
        // Default to `aspectFill` (`CENTER_CROP`) to match the Lynx
        // `mode` default; `setMode(_)` flips it as soon as a non-
        // default value lands.
        v.scaleType = ImageView.ScaleType.CENTER_CROP
        return v
    }

    /// Intercept the CSS `border-radius` cascade before the base
    /// implementation runs. Whisker-registered custom UIs ship without
    /// an APT-generated `$$PropsSetter`, so Lynx's per-key dispatch
    /// path can't reach the typed `setBorderRadius(int, ReadableArray)`
    /// hook on `LynxBaseUI`. The kebab-case `border-*-radius` entries
    /// DO reach `StylesDiffMap.mBackingMap` though — we pull them out
    /// here and forward to Coil's bitmap transformation.
    ///
    /// Lynx splits the CSS shorthand into four per-corner properties.
    /// Each value is a 4-element `[x_px, x_unit, y_px, y_unit]` array
    /// (PlatformLength quartet, x_px already density-multiplied).
    /// `RoundedCornersTransformation` takes one uniform float, so we
    /// collapse to the largest corner.
    override fun updatePropertiesInterval(props: StylesDiffMap?) {
        super.updatePropertiesInterval(props)
        val map = props?.mBackingMap ?: return
        var maxPx = 0f
        var sawAny = false
        for (k in CORNER_KEYS) {
            if (!map.hasKey(k)) continue
            val arr = map.getArray(k) ?: continue
            if (arr.size() < 1) continue
            sawAny = true
            val px = arr.getDouble(0).toFloat()
            if (px > maxPx) maxPx = px
        }
        if (sawAny && maxPx != cornerRadiusPx) {
            cornerRadiusPx = maxPx
            reload()
        }
    }

    /**
     * Backing of the `src` prop. Kicks off a Coil request bound to
     * the ImageView itself. A second `setSrc` cancels the in-flight
     * request automatically — `ImageView.load { ... }` returns a
     * Disposable we cancel before issuing the next one.
     */
    fun setSrc(value: String) {
        // No-op on equal — avoids re-fetching on a benign re-render
        // (e.g. a parent re-renders but the src signal didn't
        // actually change). Coil would short-circuit via its memory
        // cache, but the request construction itself is non-zero.
        if (currentSrc == value) return
        currentSrc = value
        reload()
    }

    /**
     * Backing of the `mode` prop. Maps the Lynx-convention mode
     * strings onto `ImageView.ScaleType`. Unknown values fall back
     * to `aspectFill` (CENTER_CROP).
     */
    /// Backing of the `headers` prop: a JSON object of request
    /// headers. Re-fetches, because a host that answers differently
    /// per header answers differently per change.
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

    fun setMode(value: String) {
        val imageView = view ?: return
        imageView.scaleType = when (value) {
            "aspectFill" -> ImageView.ScaleType.CENTER_CROP
            "aspectFit" -> ImageView.ScaleType.FIT_CENTER
            "scaleToFill" -> ImageView.ScaleType.FIT_XY
            "center" -> ImageView.ScaleType.CENTER
            else -> ImageView.ScaleType.CENTER_CROP
        }
    }

    /**
     * Issue (or re-issue) a Coil request for the current `src` with
     * the current `cornerRadiusPx`. Called from `setSrc` and from
     * `updatePropertiesInterval` when the radius changes.
     */
    private fun reload() {
        val src = currentSrc ?: return
        val imageView = view ?: return

        // Cancel any prior request. `dispose()` is a no-op if the
        // disposable has already completed.
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
            if (cornerRadiusPx > 0f) {
                transformations(RoundedCornersTransformation(cornerRadiusPx))
            }
        }
    }

    private companion object {
        val CORNER_KEYS = listOf(
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        )
    }
}
