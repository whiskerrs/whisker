// Lynx UI subclass hosting a `WhiskerSvgDrawingView`. A plain
// `WhiskerUI` subclass — no Whisker annotations, registration
// driven by `SvgModule`'s `definition()`.

package rs.whisker.modules.svg

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.util.Log
import android.view.View
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class WhiskerSvgView(context: WhiskerContext) : WhiskerUI<WhiskerSvgDrawingView>(context) {

    override fun createView(context: Context): WhiskerSvgDrawingView =
        WhiskerSvgDrawingView(context).apply {
            setBackgroundColor(Color.TRANSPARENT)
        }

    /**
     * Backing of the `_display_list` Prop. The value is the
     * Rust producer's `whisker_svg::compile()` output,
     * base64-encoded. Empty string clears the cached bytes
     * (renders nothing).
     */
    fun setDisplayList(value: String) {
        val v = view ?: return
        if (value.isEmpty()) {
            v.displayListBytes = null
            v.invalidate()
            return
        }
        val decoded = try {
            android.util.Base64.decode(value, android.util.Base64.DEFAULT)
        } catch (e: IllegalArgumentException) {
            Log.w("WhiskerSvg", "invalid base64 display list: ${e.message}")
            null
        }
        v.displayListBytes = decoded
        v.invalidate()
    }

    /**
     * Backing of the `color` Prop. Parsed as a CSS-style colour
     * string. The resolved ARGB int is substituted wherever the
     * source SVG used `fill="currentColor"` / `stroke="currentColor"`
     * (= the `FILL_TINT` / `STROKE_TINT` opcodes).
     */
    fun setColor(value: String) {
        val v = view ?: return
        v.tintArgb = parseCssColor(value)
        v.invalidate()
    }
}

/**
 * `View` that paints the cached display-list bytes inside its own
 * bounds. Lives outside `WhiskerSvgView` because Whisker's UI
 * owner expects a single `view` accessor — we keep the
 * drawing-specific `onDraw` separate from the LynxUI bookkeeping.
 */
class WhiskerSvgDrawingView(context: Context) : View(context) {

    /** Display-list payload set by [WhiskerSvgView.setDisplayList]. */
    var displayListBytes: ByteArray? = null
        set(v) { field = v; invalidate() }

    /** CSS `color` resolved value used for `FILL_TINT` /
     *  `STROKE_TINT`. Default = the platform "primary text"
     *  colour so an unstyled `<Svg>` lands on a sane neutral. */
    var tintArgb: Int = 0xFF000000.toInt()
        set(v) { field = v; invalidate() }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val bytes = displayListBytes ?: return
        if (width <= 0 || height <= 0) return
        val visitor = CanvasVisitor(
            canvas = canvas,
            tintArgb = tintArgb,
            viewWidth = width.toFloat(),
            viewHeight = height.toFloat(),
        )
        try {
            dlReplay(bytes, visitor)
        } catch (e: DLReplayError) {
            // Malformed stream — fail closed by drawing nothing
            // rather than throwing inside onDraw (the framework
            // doesn't propagate). Rust producer contract is
            // that bytes are always well-formed; if they're not,
            // that's a Whisker-side bug worth surfacing via
            // diagnostics rather than crashing the host.
            Log.w("WhiskerSvg", "replay failed: ${e.message}")
        }
    }
}

/**
 * Best-effort CSS colour parser. Supports `#RGB`, `#RRGGBB`,
 * `#RRGGBBAA`, `rgb(…)`, `rgba(…)`, plus the small named colours the
 * Rust compiler accepts. Returns the resolved ARGB int, or falls back
 * to opaque black on parse failure.
 */
private fun parseCssColor(raw: String): Int {
    val s = raw.trim()
    parseRgbFunction(s)?.let { return it }
    if (s.startsWith("#")) {
        val hex = s.substring(1)
        when (hex.length) {
            3 -> {
                val r = hex[0].digitToIntOrNull(16) ?: return 0xFF000000.toInt()
                val g = hex[1].digitToIntOrNull(16) ?: return 0xFF000000.toInt()
                val b = hex[2].digitToIntOrNull(16) ?: return 0xFF000000.toInt()
                return ((0xFF shl 24)
                    or ((r * 17) shl 16)
                    or ((g * 17) shl 8)
                    or (b * 17))
            }
            6 -> {
                val n = hex.toLongOrNull(16) ?: return 0xFF000000.toInt()
                return (0xFF000000.toInt()) or n.toInt()
            }
            8 -> {
                val n = hex.toLongOrNull(16) ?: return 0xFF000000.toInt()
                val r = ((n ushr 24) and 0xFF).toInt()
                val g = ((n ushr 16) and 0xFF).toInt()
                val b = ((n ushr 8) and 0xFF).toInt()
                val a = (n and 0xFF).toInt()
                return (a shl 24) or (r shl 16) or (g shl 8) or b
            }
        }
    }
    return when (s.lowercase()) {
        "black" -> 0xFF000000.toInt()
        "white" -> 0xFFFFFFFF.toInt()
        "red" -> 0xFFFF0000.toInt()
        "green" -> 0xFF008000.toInt()
        "blue" -> 0xFF0000FF.toInt()
        "transparent" -> 0x00000000
        else -> 0xFF000000.toInt()
    }
}

/**
 * Parses `rgb(r, g, b)` / `rgba(r, g, b, a)` — the format
 * `whisker-css`'s `Color::to_css_string()` actually emits for any
 * non-hex-literal, non-named color (e.g. every `Color::hex(...)`
 * constant reactively interpolated into a string, as opposed to a
 * hardcoded hex literal). Without this, those colors fell through to
 * the opaque-black fallback below — silently discarding the app's
 * intended color.
 */
private fun parseRgbFunction(s: String): Int? {
    val isRgba = s.startsWith("rgba(")
    if (!isRgba && !s.startsWith("rgb(")) return null
    if (!s.endsWith(")")) return null
    val inner = s.substring(if (isRgba) 5 else 4, s.length - 1)
    val parts = inner.split(",").map { it.trim() }
    if (parts.size != if (isRgba) 4 else 3) return null
    val r = parts[0].toDoubleOrNull() ?: return null
    val g = parts[1].toDoubleOrNull() ?: return null
    val b = parts[2].toDoubleOrNull() ?: return null
    val a = if (isRgba) (parts[3].toDoubleOrNull() ?: 1.0) else 1.0
    return ((a * 255).toInt() and 0xFF shl 24) or
        (r.toInt() and 0xFF shl 16) or
        (g.toInt() and 0xFF shl 8) or
        (b.toInt() and 0xFF)
}
