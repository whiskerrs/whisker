package rs.whisker.runtime

import android.graphics.Rect
import android.text.Layout
import android.text.Spannable
import android.text.Spanned
import android.text.TextPaint
import android.text.style.MetricAffectingSpan
import kotlin.math.max

private class BaselineShiftSpan(private val shift: Int) : MetricAffectingSpan() {
    override fun updateMeasureState(paint: TextPaint) { paint.baselineShift = -shift }
    override fun updateDrawState(paint: TextPaint) = updateMeasureState(paint)
}

private data class AlignmentItem(
    val start: Int,
    val end: Int,
    val ascent: Float,
    val descent: Float,
    val alignment: Int,
    val shift: Float,
    val attachment: WhiskerInlineSpan?,
)

public fun WhiskerParagraph.resolveVerticalAlignment(layout: Layout, density: Float): Boolean {
    val text = layout.text as? Spannable ?: return false
    if (runs.none { it.alignment != 0 } && attachments.none { it.alignment != 0 }) return false
    text.getSpans(0, text.length, BaselineShiftSpan::class.java).forEach(text::removeSpan)
    val root = layout.paint.fontMetrics
    val x = Rect().also { layout.paint.getTextBounds("x", 0, 1, it) }
    for (line in 0 until layout.lineCount) {
        val start = layout.getLineStart(line)
        val end = layout.getLineEnd(line)
        val items = mutableListOf<AlignmentItem>()
        var offset = start
        while (offset < end) {
            val next = text.nextSpanTransition(offset, end, MetricAffectingSpan::class.java)
            val spans = text.getSpans(offset, next, MetricAffectingSpan::class.java)
            val paint = TextPaint(layout.paint)
            spans.filterIsInstance<ParagraphSpan>().forEach { it.updateMeasureState(paint) }
            val span = spans.filterIsInstance<WhiskerInlineSpan>().firstOrNull()
            val run = runs.firstOrNull { offset >= it.start && offset < it.end }
            val attachment = span?.attachment
            val font = paint.fontMetrics
            items += AlignmentItem(offset, next, attachment?.let { it.baseline * density } ?: -font.ascent,
                attachment?.let { (it.height - it.baseline) * density } ?: font.descent,
                attachment?.alignment ?: run?.alignment ?: 0,
                (attachment?.shift ?: run?.shift ?: 0f) * density, span)
            offset = next
        }
        var ascent = -root.ascent
        var descent = root.descent
        for (item in items.filter { it.alignment != 1 && it.alignment != 3 }) {
            val shift = when (item.alignment) {
                2 -> (item.descent - item.ascent - x.top) / 2
                4 -> item.shift
                else -> 0f
            }
            ascent = max(ascent, item.ascent + shift)
            descent = max(descent, item.descent - shift)
        }
        val height = max(ascent + descent, items.filter { it.alignment == 1 || it.alignment == 3 }.maxOfOrNull { it.ascent + it.descent } ?: 0f)
        val leading = (height - ascent - descent) / 2
        ascent += leading
        descent += leading
        for (item in items) {
            val shift = when (item.alignment) {
                1 -> ascent - item.ascent
                2 -> (item.descent - item.ascent - x.top) / 2
                3 -> item.descent - descent
                4 -> item.shift
                else -> 0f
            }
            if (item.attachment != null) item.attachment.alignedBaseline = item.ascent + shift
            else text.setSpan(BaselineShiftSpan(shift.toInt()), item.start, item.end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
    }
    return true
}
