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
import rs.whisker.runtime.WhiskerParagraph
import rs.whisker.runtime.measurementGeometry
import rs.whisker.runtime.truncateToFit
import rs.whisker.runtime.resolveVerticalAlignment
import rs.whisker.runtime.resolveWhiskerTypeface
import rs.whisker.runtime.internal.CenteredLineHeightSpan

/** Intrinsic measurement implementation shared by all Android Host frames. */
internal class HostMeasurementProvider(
    private val context: Context,
    private val elements: WhiskerElementBindings,
    internal val preparedParagraphs: PreparedParagraphs = PreparedParagraphs(),
) {
    private val layouts = HashMap<TextLayoutKey, TextLayout>()
    private val paints = HashMap<TextPaintKey, TextPaint>()

    @Suppress("LongParameterList")
    fun measure(
        node: Long, environmentEpoch: Long,
        elementType: Int, kind: Int,
        knownWidth: Float, knownHeight: Float, knownMask: Int,
        availableWidth: Float, availableHeight: Float,
        availableWidthKind: Int, availableHeightKind: Int,
        text: String, locale: String, fontFamilies: Array<String>, fontSize: Float, fontWeight: Int,
        fontStyle: Int, wrap: Int, wordBreak: Int, overflow: Int, letterSpacing: Float,
        lineHeight: Float, indentLogicalPixels: Float, indentPercentage: Float,
        maxLines: Int, fontSettings: Array<String>, fontFeatureCount: Int,
        fontOpticalSizing: Int, payloadVersion: Int, payload: WhiskerValue?,
        intrinsicWidth: Float, intrinsicHeight: Float, intrinsicMask: Int,
        direction: Int, alignment: Int,
        paragraph: ByteArray? = null,
    ): HostMeasurementResult {
        if (kind == MEASURE_TEXT) {
            val density = context.resources.displayMetrics.density
            val localeRtl = context.resources.configuration.layoutDirection == View.LAYOUT_DIRECTION_RTL
            val widthBasis = when {
                knownMask and WIDTH != 0 -> knownWidth
                availableWidthKind == DEFINITE -> availableWidth
                else -> 0f
            }
            val key = TextLayoutKey(
                // Views restyle the layout they draw in place, so rich layouts stay per node.
                if (paragraph != null) node else 0L,
                environmentEpoch, density, localeRtl,
                when {
                    knownMask and WIDTH != 0 -> ceil(knownWidth * density).toInt().coerceAtLeast(1)
                    availableWidthKind == DEFINITE && wrap != 0 -> ceil(availableWidth * density).toInt().coerceAtLeast(1)
                    availableWidthKind == MIN_CONTENT && wrap != 0 -> 1
                    else -> DESIRED_WIDTH
                },
                text, locale, fontFamilies.asList(), fontSize, fontWeight, fontStyle, wrap, wordBreak, overflow,
                letterSpacing, lineHeight, textIndentPixels(widthBasis, density, indentLogicalPixels, indentPercentage),
                maxLines, fontSettings.asList(), fontFeatureCount, fontOpticalSizing, direction, alignment,
                paragraph?.let(::EncodedParagraph),
            )
            val layout = layouts[key] ?: layoutText(key).also {
                if (layouts.size >= LAYOUT_CACHE_CAPACITY) layouts.clear()
                layouts[key] = it
            }
            val width = measuredTextWidth(
                knownWidth = knownWidth,
                hasKnownWidth = knownMask and WIDTH != 0,
                availableWidth = availableWidth,
                hasDefiniteAvailableWidth = availableWidthKind == DEFINITE,
                wraps = wrap != 0,
                usedLineWidthPixels = layout.usedLineWidthPixels,
                density = density,
            )
            return HostMeasurementResult(
                width = width,
                height = if (knownMask and HEIGHT != 0) knownHeight else layout.height,
                firstBaseline = layout.firstBaseline,
                lastBaseline = layout.lastBaseline,
                mask = BASELINES,
                paragraph = layout.geometry,
                layout = layout.layout.takeIf { paragraph != null },
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
                payload ?: WhiskerValue.Null,
            ),
        ) ?: return HostMeasurementResult(status = UNSUPPORTED, reason = UNSUPPORTED_FEATURE)
        return ready(
            if (knownMask and WIDTH != 0) knownWidth else custom.width,
            if (knownMask and HEIGHT != 0) knownHeight else custom.height,
        )
    }

    private fun layoutText(key: TextLayoutKey): TextLayout = with(key) {
        val rich = paragraph?.let { WhiskerParagraph.decode(ValueBytes.decode(it.bytes), text) }
        val paintKey = TextPaintKey(density, locale, fontFamilies, fontSize, fontWeight, fontStyle, letterSpacing, fontSettings, fontFeatureCount, fontOpticalSizing)
        val prototype = paints[paintKey] ?: createPaint(paintKey).also {
            if (paints.size >= PAINT_CACHE_CAPACITY) paints.clear()
            paints[paintKey] = it
        }
        // Views recolor the paint of the layout they draw, so each layout owns a copy.
        val paint = TextPaint(prototype)
        val displayText = if (rich == null && wordBreak == WORD_BREAK_KEEP_ALL) protectCjkBreaks(text) else text
        val semantics = resolveTextLayoutSemantics(displayText, direction, alignment, localeRtl, indentPixels)
        fun styledText(value: String, paragraph: WhiskerParagraph?, width: Float = Float.POSITIVE_INFINITY): CharSequence {
            if (value.isEmpty() || (paragraph == null && semantics.indentPixels == 0f && lineHeight <= 0f)) return value
            return SpannableString(value).apply {
                paragraph?.apply(this, density, width)
                if (semantics.indentPixels != 0f) setSpan(LeadingMarginSpan.Standard(semantics.indentPixels.toInt(), 0), 0, length, Spanned.SPAN_INCLUSIVE_EXCLUSIVE)
                if (lineHeight > 0f) setSpan(CenteredLineHeightSpan(lineHeight * density), 0, length, Spanned.SPAN_INCLUSIVE_EXCLUSIVE)
            }
        }
        val layoutText = styledText(displayText, rich)
        val maxWidthPx = if (widthPixels != DESIRED_WIDTH) widthPixels else {
            ceil(Layout.getDesiredWidth(layoutText, paint) + semantics.indentPixels).toInt().coerceAtLeast(1)
        }
        val lineLimit = if (wrap == 0) 1 else if (maxLines == 0) Int.MAX_VALUE else maxLines
        fun buildLayout(value: CharSequence, limit: Int = lineLimit, ellipsis: Boolean = overflow == TEXT_OVERFLOW_ELLIPSIS): StaticLayout {
            fun shape(): StaticLayout = configureFallbackLineSpacing(StaticLayout.Builder.obtain(value, 0, value.length, paint, maxWidthPx))
                .setAlignment(semantics.alignment)
                .setTextDirection(semantics.directionHeuristic)
                .setIncludePad(Build.VERSION.SDK_INT < Build.VERSION_CODES.P)
                .setMaxLines(limit)
                .setBreakStrategy(if (wordBreak == WORD_BREAK_BREAK_ALL) Layout.BREAK_STRATEGY_SIMPLE else Layout.BREAK_STRATEGY_HIGH_QUALITY)
                .apply { if (ellipsis) setEllipsize(TextUtils.TruncateAt.END).setEllipsizedWidth(maxWidthPx) }
                .build()
            val initial = shape()
            return if (rich?.resolveVerticalAlignment(initial, density) == true) shape() else initial
        }
        var measuredParagraph = rich
        val layout = if (rich?.attachments?.any { it.truncation } == true) {
            fun fits(layout: StaticLayout): Boolean = layout.lineCount <= lineLimit && (0 until layout.lineCount).all { layout.getLineWidth(it) <= maxWidthPx }
            val full = buildLayout(layoutText, Int.MAX_VALUE, false)
            if (fits(full)) full else {
                val (truncated, paragraph) = rich.truncateToFit(
                    fits = ::fits,
                    layout = { paragraph -> buildLayout(styledText(paragraph.displayText(), paragraph, maxWidthPx / density), Int.MAX_VALUE, false) },
                )
                measuredParagraph = paragraph
                truncated
            }
        } else buildLayout(layoutText)
        val first = if (layout.lineCount > 0) layout.getLineBaseline(0) / density else 0f
        TextLayout(
            layout = layout,
            usedLineWidthPixels = (0 until layout.lineCount).maxOfOrNull(layout::getLineWidth) ?: 0f,
            height = layout.height / density,
            firstBaseline = first,
            lastBaseline = if (layout.lineCount > 0) layout.getLineBaseline(layout.lineCount - 1) / density else first,
            geometry = measuredParagraph?.measurementGeometry(layout, density)?.let(ValueBytes::encode),
        )
    }

    // TextPaint's own `density` and `letterSpacing` would shadow an implicit key receiver here.
    private fun createPaint(key: TextPaintKey): TextPaint = TextPaint().apply {
        textSize = key.fontSize * key.density
        if (key.locale.isNotEmpty()) textLocale = Locale.forLanguageTag(key.locale)
        val italic = key.fontStyle != FONT_STYLE_NORMAL
        val baseTypeface = resolveWhiskerTypeface(key.fontFamilies)
        typeface = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            Typeface.create(baseTypeface, key.fontWeight.coerceIn(1, 1000), italic)
        } else {
            val typefaceStyle = (if (key.fontWeight >= 600) Typeface.BOLD else 0) or
                (if (italic) Typeface.ITALIC else 0)
            Typeface.create(baseTypeface, typefaceStyle)
        }
        letterSpacing = if (key.fontSize > 0f) key.letterSpacing / key.fontSize else 0f
        fontFeatureSettings = key.fontSettings.take(key.fontFeatureCount).joinToString(", ") {
            val (tag, value) = parseFontSetting(it)
            "'$tag' ${value.toLong()}"
        }.ifEmpty { null }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val variations = key.fontSettings.drop(key.fontFeatureCount)
                .map(::parseFontSetting)
                .toMutableList()
            if (key.fontOpticalSizing == 0 && variations.none { it.first == "opsz" }) {
                variations += "opsz" to key.fontSize.toDouble()
            }
            fontVariationSettings = variations.joinToString(", ") {
                "'${it.first}' ${it.second}"
            }.ifEmpty { null }
        }
    }

    private fun ready(width: Float, height: Float): HostMeasurementResult =
        HostMeasurementResult(width = width, height = height)
}

internal fun configureFallbackLineSpacing(
    builder: StaticLayout.Builder,
): StaticLayout.Builder {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        builder.setUseLineSpacingFromFallbacks(true)
    }
    return builder
}

internal fun measuredTextWidth(
    knownWidth: Float,
    hasKnownWidth: Boolean,
    availableWidth: Float,
    hasDefiniteAvailableWidth: Boolean,
    wraps: Boolean,
    usedLineWidthPixels: Float,
    density: Float,
): Float = when {
    hasKnownWidth -> knownWidth
    hasDefiniteAvailableWidth && wraps ->
        (ceil(usedLineWidthPixels.toDouble()).toFloat() / density).coerceAtMost(availableWidth)
    else -> ceil(usedLineWidthPixels.toDouble()).toFloat() / density
}

internal data class TextLayoutSemantics(
    val alignment: Layout.Alignment,
    val directionHeuristic: TextDirectionHeuristic,
    val indentPixels: Float,
)

internal fun textIndentPixels(
    widthBasis: Float,
    density: Float,
    indentLogicalPixels: Float,
    indentPercentage: Float,
): Float = indentLogicalPixels * density + widthBasis * density * indentPercentage / 100f

internal fun resolveTextLayoutSemantics(
    text: String,
    direction: Int,
    alignment: Int,
    localeRtl: Boolean,
    indentPixels: Float,
): TextLayoutSemantics {
    val rightToLeft = resolvesRightToLeft(text, direction, localeRtl)
    return TextLayoutSemantics(
        alignment = resolveAlignment(alignment, rightToLeft),
        directionHeuristic = resolveDirectionHeuristic(direction, localeRtl),
        indentPixels = indentPixels,
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

private const val DESIRED_WIDTH = -1
private const val LAYOUT_CACHE_CAPACITY = 256
private const val PAINT_CACHE_CAPACITY = 64
private const val DEFINITE = 0
private const val MIN_CONTENT = 1
private const val WIDTH = 1
private const val HEIGHT = 2
private const val BOTH_DIMENSIONS = WIDTH or HEIGHT
private const val MEASURE_TEXT = 1
private const val MEASURE_REPLACED_CONTENT = 2
private const val MEASURE_EMBEDDED_SURFACE = 4
private const val FONT_STYLE_NORMAL = 0
private const val READY = 1
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
private const val UNSUPPORTED = 3
private const val UNSUPPORTED_FEATURE = 1
private const val BASELINES = 3

/** Every input that shapes a text layout; equal keys produce interchangeable layouts. */
private data class TextLayoutKey(
    val node: Long,
    val environmentEpoch: Long,
    val density: Float,
    val localeRtl: Boolean,
    val widthPixels: Int,
    val text: String,
    val locale: String,
    val fontFamilies: List<String>,
    val fontSize: Float,
    val fontWeight: Int,
    val fontStyle: Int,
    val wrap: Int,
    val wordBreak: Int,
    val overflow: Int,
    val letterSpacing: Float,
    val lineHeight: Float,
    val indentPixels: Float,
    val maxLines: Int,
    val fontSettings: List<String>,
    val fontFeatureCount: Int,
    val fontOpticalSizing: Int,
    val direction: Int,
    val alignment: Int,
    val paragraph: EncodedParagraph?,
)

private data class TextPaintKey(
    val density: Float,
    val locale: String,
    val fontFamilies: List<String>,
    val fontSize: Float,
    val fontWeight: Int,
    val fontStyle: Int,
    val letterSpacing: Float,
    val fontSettings: List<String>,
    val fontFeatureCount: Int,
    val fontOpticalSizing: Int,
)

private class EncodedParagraph(val bytes: ByteArray) {
    private val hash = bytes.contentHashCode()
    override fun equals(other: Any?): Boolean = other is EncodedParagraph && hash == other.hash && bytes.contentEquals(other.bytes)
    override fun hashCode(): Int = hash
}

private class TextLayout(
    val layout: StaticLayout,
    val usedLineWidthPixels: Float,
    val height: Float,
    val firstBaseline: Float,
    val lastBaseline: Float,
    val geometry: ByteArray?,
)

internal data class HostMeasurementResult(
    val status: Int = READY,
    val reason: Int = 0,
    val width: Float = 0f,
    val height: Float = 0f,
    val firstBaseline: Float = 0f,
    val lastBaseline: Float = 0f,
    val mask: Int = 0,
    val paragraph: ByteArray? = null,
    val layout: StaticLayout? = null,
)

