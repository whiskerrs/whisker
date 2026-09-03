package rs.whisker.runtime.measure

import android.content.Context
import android.graphics.Typeface
import android.os.Build
import android.text.Layout
import android.text.SpannableString
import android.text.Spanned
import android.text.StaticLayout
import android.text.TextPaint
import android.text.TextDirectionHeuristic
import android.text.TextDirectionHeuristics
import android.text.TextUtils
import android.text.style.LeadingMarginSpan
import android.view.View
import java.text.Bidi
import java.util.Locale
import kotlin.math.ceil
import rs.whisker.runtime.WhiskerAvailableSpace
import rs.whisker.runtime.WhiskerElementBindings
import rs.whisker.runtime.WhiskerMeasureRequest
import rs.whisker.runtime.WhiskerValue
import rs.whisker.runtime.resolveWhiskerTypeface

/** Intrinsic measurement implementation shared by all Android Host frames. */
internal class HostMeasurementProvider(
    private val context: Context,
    private val elements: WhiskerElementBindings,
) {
    @Suppress("LongParameterList")
    fun measure(
        elementType: Int, kind: Int,
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableHeight: Float,
        availableWidthKind: Int, availableHeightKind: Int,
        text: String, locale: String, fontFamilies: Array<String>, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float,
        maxLines: Int, fontSettings: Array<String>, fontFeatureCount: Int,
        fontOpticalSizing: Int, payloadVersion: Int, payload: ByteArray,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
        direction: Int, alignment: Int,
    ): FloatArray {
        if (kind == MEASURE_TEXT) {
            return measureText(
                knownWidth, knownHeight, knownMask,
                availableWidth, availableWidthKind,
                text, locale, fontFamilies, fontSize, fontWeight, fontStyle, wrap, wordBreak, overflow,
                letterSpacing, lineHeight, indentLogicalPixels, indentPercentage, maxLines,
                fontSettings, fontFeatureCount, fontOpticalSizing, direction, alignment,
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
        val custom = elements.measure(
            elementType,
            WhiskerMeasureRequest(
                if (availableWidthKind == DEFINITE) availableWidth else null,
                if (availableHeightKind == DEFINITE) availableHeight else null,
                WhiskerAvailableSpace.entries[availableWidthKind],
                WhiskerAvailableSpace.entries[availableHeightKind],
                if (knownMask and WIDTH != 0) knownWidth else null,
                if (knownMask and HEIGHT != 0) knownHeight else null,
                payloadVersion,
                WhiskerValue.Bytes(payload),
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
        text: String, locale: String, fontFamilies: Array<String>, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float, maxLines: Int,
        fontSettings: Array<String>, fontFeatureCount: Int, fontOpticalSizing: Int,
        direction: Int, alignment: Int,
    ): FloatArray {
        val density = context.resources.displayMetrics.density
        val paint = TextPaint().apply {
            textSize = fontSize * density
            if (locale.isNotEmpty()) textLocale = Locale.forLanguageTag(locale)
            val italic = fontStyle != FONT_STYLE_NORMAL
            val baseTypeface = resolveWhiskerTypeface(fontFamilies.asList())
            typeface = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                Typeface.create(baseTypeface, fontWeight.coerceIn(1, 1000), italic)
            } else {
                val typefaceStyle = (if (fontWeight >= 600) Typeface.BOLD else 0) or
                    (if (italic) Typeface.ITALIC else 0)
                Typeface.create(baseTypeface, typefaceStyle)
            }
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
        val displayText = if (wordBreak == WORD_BREAK_KEEP_ALL) protectCjkBreaks(text) else text
        val localeRtl = context.resources.configuration.layoutDirection == View.LAYOUT_DIRECTION_RTL
        val semantics = resolveTextLayoutSemantics(
            displayText,
            direction,
            alignment,
            localeRtl,
            widthBasis,
            density,
            indentLogicalPixels,
            indentPercentage,
        )
        val layoutText: CharSequence = if (displayText.isEmpty() || semantics.indentPixels == 0f) {
            displayText
        } else {
            SpannableString(displayText).apply {
                setSpan(
                    LeadingMarginSpan.Standard(semantics.indentPixels.toInt(), 0),
                    0,
                    length,
                    Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
                )
            }
        }
        val maxWidthPx = if (availableWidthKind == DEFINITE && wrap != 0) {
            ceil(availableWidth * density).toInt().coerceAtLeast(1)
        } else {
            ceil(paint.measureText(text) + semantics.indentPixels).toInt().coerceAtLeast(1)
        }
        val builder = StaticLayout.Builder.obtain(
            layoutText, 0, layoutText.length, paint, maxWidthPx,
        )
            .setAlignment(semantics.alignment)
            .setTextDirection(semantics.directionHeuristic)
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
        val width = when {
            knownMask and WIDTH != 0 -> knownWidth
            availableWidthKind == DEFINITE && wrap != 0 ->
                (layout.width / density).coerceAtMost(availableWidth)
            else -> layout.width / density
        }
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

internal data class TextLayoutSemantics(
    val alignment: Layout.Alignment,
    val directionHeuristic: TextDirectionHeuristic,
    val indentPixels: Float,
)

@Suppress("LongParameterList")
internal fun resolveTextLayoutSemantics(
    text: String,
    direction: Int,
    alignment: Int,
    localeRtl: Boolean,
    widthBasis: Float,
    density: Float,
    indentLogicalPixels: Float,
    indentPercentage: Float,
): TextLayoutSemantics {
    val rightToLeft = resolvesRightToLeft(text, direction, localeRtl)
    return TextLayoutSemantics(
        alignment = resolveAlignment(alignment, rightToLeft),
        directionHeuristic = resolveDirectionHeuristic(direction, localeRtl),
        indentPixels = indentLogicalPixels * density +
            widthBasis * density * indentPercentage / 100f,
    )
}

private fun resolveDirectionHeuristic(
    direction: Int,
    localeRtl: Boolean,
): TextDirectionHeuristic = when (direction) {
    TEXT_DIRECTION_AUTO -> if (localeRtl) {
        TextDirectionHeuristics.FIRSTSTRONG_RTL
    } else {
        TextDirectionHeuristics.FIRSTSTRONG_LTR
    }
    TEXT_DIRECTION_LEFT_TO_RIGHT -> TextDirectionHeuristics.LTR
    TEXT_DIRECTION_RIGHT_TO_LEFT -> TextDirectionHeuristics.RTL
    else -> error("unsupported text direction: $direction")
}

private fun resolvesRightToLeft(text: String, direction: Int, localeRtl: Boolean): Boolean =
    when (direction) {
        TEXT_DIRECTION_LEFT_TO_RIGHT -> false
        TEXT_DIRECTION_RIGHT_TO_LEFT -> true
        TEXT_DIRECTION_AUTO -> if (text.isEmpty()) {
            localeRtl
        } else {
            val fallback = if (localeRtl) {
                Bidi.DIRECTION_DEFAULT_RIGHT_TO_LEFT
            } else {
                Bidi.DIRECTION_DEFAULT_LEFT_TO_RIGHT
            }
            !Bidi(text, fallback).baseIsLeftToRight()
        }
        else -> error("unsupported text direction: $direction")
    }

private fun resolveAlignment(alignment: Int, rightToLeft: Boolean): Layout.Alignment =
    when (alignment) {
        TEXT_ALIGNMENT_START -> Layout.Alignment.ALIGN_NORMAL
        TEXT_ALIGNMENT_END -> Layout.Alignment.ALIGN_OPPOSITE
        TEXT_ALIGNMENT_LEFT -> if (rightToLeft) {
            Layout.Alignment.ALIGN_OPPOSITE
        } else {
            Layout.Alignment.ALIGN_NORMAL
        }
        TEXT_ALIGNMENT_RIGHT -> if (rightToLeft) {
            Layout.Alignment.ALIGN_NORMAL
        } else {
            Layout.Alignment.ALIGN_OPPOSITE
        }
        TEXT_ALIGNMENT_CENTER -> Layout.Alignment.ALIGN_CENTER
        else -> error("unsupported text alignment: $alignment")
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
private const val FONT_STYLE_NORMAL = 0
private const val READY = 1f
private const val WORD_BREAK_BREAK_ALL = 1
private const val WORD_BREAK_KEEP_ALL = 2
private const val TEXT_OVERFLOW_ELLIPSIS = 1
private const val TEXT_DIRECTION_AUTO = 0
private const val TEXT_DIRECTION_LEFT_TO_RIGHT = 1
private const val TEXT_DIRECTION_RIGHT_TO_LEFT = 2
private const val TEXT_ALIGNMENT_START = 0
private const val TEXT_ALIGNMENT_END = 1
private const val TEXT_ALIGNMENT_LEFT = 2
private const val TEXT_ALIGNMENT_RIGHT = 3
private const val TEXT_ALIGNMENT_CENTER = 4
private const val UNSUPPORTED = 3f
private const val UNSUPPORTED_FEATURE = 1f
private const val BASELINES = 3f
