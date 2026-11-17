// CSS `line-height` for the native EditText, shared by the measurer and the
// view so the two always agree on the field's height.
//
// `setLineSpacing(extra)` puts all of the extra leading below each line, so
// text sits at the top of its line box instead of in the middle as CSS
// places it. It also leaves line heights to the font metrics of whichever
// font draws the line: with fallback line spacing (the EditText default
// since API 28) a line of CJK text, drawn by the fallback font, is taller
// than the primary font's line the measurer assumed, so an auto-sized
// multiline field overflows and scrolls the top of its first line out of
// view.
//
// A LineHeightSpan pins every line to exactly `line-height`, independent of
// fallback fonts. Android, however, lays out the empty line after a trailing
// newline (and an empty text) from the paint's metrics without consulting
// spans (InputTypography keeps API 35's locale minimum off that line). To keep that line consistent, every line keeps the primary font's
// ascent as its baseline offset, the last line keeps the primary font's
// descent, and the half-leading CSS would put above the first line and below
// the last one becomes view padding. The resulting height is
// `lines * line-height` whether or not the text ends with a newline.

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
            // A line-height below the font's own height would need negative
            // padding; keep the natural height instead.
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
