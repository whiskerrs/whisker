package rs.whisker.runtime.measure

import android.content.Context
import android.graphics.Typeface
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import rs.whisker.runtime.WhiskerElementRegistry
import rs.whisker.runtime.WhiskerMeasureRequest

/** Intrinsic measurement implementation shared by all Android Host frames. */
internal class HostMeasurementProvider(private val context: Context) {
    @Suppress("LongParameterList")
    fun measure(
        elementType: Int, kind: Int,
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableHeight: Float,
        availableWidthKind: Int, availableHeightKind: Int,
        text: String, fontFamily: String, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, letterSpacing: Float,
        lineHeight: Float, maxLines: Int, payloadVersion: Int, payload: ByteArray,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
    ): FloatArray {
        if (kind == MEASURE_TEXT) {
            return measureText(
                knownWidth, knownHeight, knownMask,
                availableWidth, availableWidthKind,
                text, fontFamily, fontSize, fontWeight, fontStyle, wrap,
                letterSpacing, lineHeight, maxLines,
            )
        }
        if ((kind == MEASURE_REPLACED_CONTENT || kind == MEASURE_EMBEDDED_SURFACE) &&
            intrinsicMask == BOTH_DIMENSIONS
        ) {
            return ready(
                if (knownMask and WIDTH != 0) knownWidth else intrinsicWidth,
                if (knownMask and HEIGHT != 0) knownHeight else intrinsicHeight,
            )
        }
        val custom = WhiskerElementRegistry.measure(
            elementType,
            WhiskerMeasureRequest(
                if (availableWidthKind == DEFINITE) availableWidth else null,
                if (availableHeightKind == DEFINITE) availableHeight else null,
                if (knownMask and WIDTH != 0) knownWidth else null,
                if (knownMask and HEIGHT != 0) knownHeight else null,
                payloadVersion,
                payload,
            ),
        ) ?: return floatArrayOf(UNSUPPORTED, UNSUPPORTED_FEATURE, 0f, 0f, 0f, 0f, 0f)
        return ready(
            if (knownMask and WIDTH != 0) knownWidth else custom.width,
            if (knownMask and HEIGHT != 0) knownHeight else custom.height,
        )
    }

    @Suppress("LongParameterList")
    private fun measureText(
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableWidthKind: Int,
        text: String, fontFamily: String, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, letterSpacing: Float,
        lineHeight: Float, maxLines: Int,
    ): FloatArray {
        val density = context.resources.displayMetrics.density
        val paint = TextPaint().apply {
            textSize = fontSize * density
            val typefaceStyle = (if (fontWeight >= 600) Typeface.BOLD else 0) or
                (if (fontStyle != 0) Typeface.ITALIC else 0)
            val baseTypeface = if (fontFamily.isEmpty()) Typeface.DEFAULT else
                Typeface.create(fontFamily, Typeface.NORMAL)
            typeface = Typeface.create(baseTypeface, typefaceStyle)
            this.letterSpacing = if (fontSize > 0f) letterSpacing / fontSize else 0f
        }
        val maxWidthPx = if (availableWidthKind == DEFINITE && wrap != 0) {
            (availableWidth * density).toInt().coerceAtLeast(1)
        } else {
            paint.measureText(text).toInt().coerceAtLeast(1)
        }
        val builder = StaticLayout.Builder.obtain(text, 0, text.length, paint, maxWidthPx)
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setIncludePad(false)
            .setMaxLines(if (maxLines == 0) Int.MAX_VALUE else maxLines)
        if (lineHeight > 0f) {
            val fontHeight = paint.fontMetrics.run { descent - ascent }
            builder.setLineSpacing((lineHeight * density - fontHeight).coerceAtLeast(0f), 1f)
        }
        val layout = builder.build()
        val width = if (knownMask and WIDTH != 0) knownWidth else layout.width / density
        val height = if (knownMask and HEIGHT != 0) knownHeight else layout.height / density
        val first = if (layout.lineCount > 0) layout.getLineBaseline(0) / density else 0f
        val last = if (layout.lineCount > 0) {
            layout.getLineBaseline(layout.lineCount - 1) / density
        } else {
            first
        }
        return floatArrayOf(READY, 0f, width, height, first, last, BASELINES)
    }

    private fun ready(width: Float, height: Float): FloatArray =
        floatArrayOf(READY, 0f, width, height, 0f, 0f, 0f)
}

private const val DEFINITE = 0
private const val WIDTH = 1
private const val HEIGHT = 2
private const val BOTH_DIMENSIONS = WIDTH or HEIGHT
private const val MEASURE_TEXT = 1
private const val MEASURE_REPLACED_CONTENT = 2
private const val MEASURE_EMBEDDED_SURFACE = 4
private const val READY = 1f
private const val UNSUPPORTED = 3f
private const val UNSUPPORTED_FEATURE = 1f
private const val BASELINES = 3f
