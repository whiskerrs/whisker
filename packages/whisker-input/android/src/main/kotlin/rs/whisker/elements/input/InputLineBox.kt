package rs.whisker.elements.input

import android.graphics.Paint
import android.text.Spannable
import android.text.Spanned
import android.text.TextPaint
import android.text.style.LineHeightSpan
import android.text.style.UpdateLayout
import kotlin.math.roundToInt

/** Resolved line box for one font and `line-height`, in physical pixels. */
internal data class InputLineBox(
    val lineHeight: Int,
    val ascent: Int,
    val descent: Int,
) {
    private val leading: Int get() = lineHeight - (descent - ascent)

    /** Half-leading above the first line. */
    val paddingTop: Int get() = leading / 2

    /** Half-leading below the last line. */
    val paddingBottom: Int get() = leading - paddingTop

    companion object {
        /** Returns null for `line-height: normal`. */
        fun of(paint: TextPaint, lineHeightPixels: Float?): InputLineBox? {
            if (lineHeightPixels == null || lineHeightPixels <= 0f) return null
            val metrics = paint.fontMetricsInt
            val natural = metrics.descent - metrics.ascent
            // Android view padding cannot represent negative half-leading.
            val lineHeight = lineHeightPixels.roundToInt().coerceAtLeast(natural)
            return InputLineBox(lineHeight, metrics.ascent, metrics.descent)
        }
    }
}

/**
 * Pins each line to [InputLineBox.lineHeight], except the last line, which
 * keeps the primary font's height to match the span-less empty line Android
 * appends after a trailing newline.
 *
 * Implements [UpdateLayout] so the EditText's DynamicLayout reflows when the
 * span is attached or moved.
 */
internal class InputLineHeightSpan(val box: InputLineBox) : LineHeightSpan, UpdateLayout {
    override fun chooseHeight(
        text: CharSequence?,
        start: Int,
        end: Int,
        spanstartv: Int,
        lineHeight: Int,
        fm: Paint.FontMetricsInt,
    ) {
        val descent = if (text == null || isLastInputLine(text, end)) {
            box.descent
        } else {
            box.ascent + box.lineHeight
        }
        fm.ascent = box.ascent
        fm.top = box.ascent
        fm.descent = descent
        fm.bottom = descent
        fm.leading = 0
    }
}

/** True when the line ending at [end] is the final line of [text]. */
internal fun isLastInputLine(text: CharSequence, end: Int): Boolean =
    end >= text.length && (text.isEmpty() || text[text.length - 1] != '\n')

/**
 * Makes [text] carry exactly one [InputLineHeightSpan] for [box] spanning the
 * whole text, or none when [box] is null. Leaves a matching span untouched so
 * the layout is not reflowed needlessly.
 */
internal fun applyInputLineBox(text: Spannable, box: InputLineBox?) {
    val spans = text.getSpans(0, text.length, InputLineHeightSpan::class.java)
    if (box != null && spans.size == 1) {
        val span = spans[0]
        if (span.box == box && text.getSpanStart(span) == 0 && text.getSpanEnd(span) == text.length) return
    }
    spans.forEach(text::removeSpan)
    if (box != null) {
        text.setSpan(InputLineHeightSpan(box), 0, text.length, Spanned.SPAN_INCLUSIVE_INCLUSIVE)
    }
}
