package rs.whisker.runtime.measure

import android.content.Context
import android.graphics.Typeface
import android.os.Build
import android.text.Layout
import android.text.SpannableString
import android.text.Spanned
import android.text.StaticLayout
import android.text.TextPaint
import android.text.TextUtils
import android.text.style.LeadingMarginSpan
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
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float,
        maxLines: Int, fontSettings: Array<String>, fontFeatureCount: Int,
        fontOpticalSizing: Int, payloadVersion: Int, payload: ByteArray,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
    ): FloatArray {
        if (kind == MEASURE_TEXT) {
            return measureText(
                knownWidth, knownHeight, knownMask,
                availableWidth, availableWidthKind,
                text, fontFamily, fontSize, fontWeight, fontStyle, wrap, wordBreak, overflow,
                letterSpacing, lineHeight, indentLogicalPixels, indentPercentage, maxLines,
                fontSettings, fontFeatureCount, fontOpticalSizing,
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
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float, maxLines: Int,
        fontSettings: Array<String>, fontFeatureCount: Int, fontOpticalSizing: Int,
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
            fontFeatureSettings = fontSettings.take(fontFeatureCount).joinToString(", ") {
                val (tag, value) = parseFontSetting(it)
                "'$tag' ${value.toLong()}"
            }.ifEmpty { null }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val variations = fontSettings.drop(fontFeatureCount)
                    .map(::parseFontSetting)
                    .toMutableList()
                if (fontOpticalSizing == 0 && variations.none { it.first == "opsz" }) {
                    variations += "opsz" to fontSize.toDouble()
                }
                fontVariationSettings = variations.joinToString(", ") {
                    "'${it.first}' ${it.second}"
                }.ifEmpty { null }
            }
        }
        val widthBasis = when {
            knownMask and WIDTH != 0 -> knownWidth
            availableWidthKind == DEFINITE -> availableWidth
            else -> 0f
        }
        val indentPixels = indentLogicalPixels * density +
            widthBasis * density * indentPercentage / 100f
        val displayText = if (wordBreak == WORD_BREAK_KEEP_ALL) protectCjkBreaks(text) else text
        val layoutText: CharSequence = if (displayText.isEmpty() || indentPixels == 0f) {
            displayText
        } else {
            SpannableString(displayText).apply {
                setSpan(
                    LeadingMarginSpan.Standard(indentPixels.toInt(), 0),
                    0,
                    length,
                    Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
                )
            }
        }
        val maxWidthPx = if (availableWidthKind == DEFINITE && wrap != 0) {
            (availableWidth * density).toInt().coerceAtLeast(1)
        } else {
            (paint.measureText(text) + indentPixels).toInt().coerceAtLeast(1)
        }
        val builder = StaticLayout.Builder.obtain(
            layoutText, 0, layoutText.length, paint, maxWidthPx,
        )
            .setAlignment(Layout.Alignment.ALIGN_NORMAL)
            .setIncludePad(false)
            .setMaxLines(if (maxLines == 0) Int.MAX_VALUE else maxLines)
            .setBreakStrategy(
                if (wordBreak == WORD_BREAK_BREAK_ALL) Layout.BREAK_STRATEGY_SIMPLE
                else Layout.BREAK_STRATEGY_HIGH_QUALITY,
            )
        if (overflow == TEXT_OVERFLOW_ELLIPSIS) {
            builder.setEllipsize(TextUtils.TruncateAt.END).setEllipsizedWidth(maxWidthPx)
        }
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

private fun parseFontSetting(value: String): Pair<String, Double> {
    val separator = value.indexOf('=')
    require(separator == 4)
    return value.substring(0, separator) to value.substring(separator + 1).toDouble()
}

private fun protectCjkBreaks(value: String): String = buildString {
    var previousWasCjk = false
    value.forEach { character ->
        val currentIsCjk = character.isCjk()
        if (previousWasCjk && currentIsCjk) append('\u2060')
        append(character)
        previousWasCjk = currentIsCjk
    }
}

private fun Char.isCjk(): Boolean = code in 0x2E80..0x9FFF || code in 0xF900..0xFAFF ||
    code in 0xAC00..0xD7AF

private const val DEFINITE = 0
private const val WIDTH = 1
private const val HEIGHT = 2
private const val BOTH_DIMENSIONS = WIDTH or HEIGHT
private const val MEASURE_TEXT = 1
private const val MEASURE_REPLACED_CONTENT = 2
private const val MEASURE_EMBEDDED_SURFACE = 4
private const val READY = 1f
private const val WORD_BREAK_BREAK_ALL = 1
private const val WORD_BREAK_KEEP_ALL = 2
private const val TEXT_OVERFLOW_ELLIPSIS = 1
private const val UNSUPPORTED = 3f
private const val UNSUPPORTED_FEATURE = 1f
private const val BASELINES = 3f
