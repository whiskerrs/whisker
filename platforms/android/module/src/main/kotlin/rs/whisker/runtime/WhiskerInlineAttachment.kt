package rs.whisker.runtime

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.text.Layout
import android.text.Spanned
import android.text.style.ReplacementSpan
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.min

public data class WhiskerInlineAttachment(
    public val node: Long,
    public val start: Int,
    public val end: Int,
    public val width: Float,
    public val height: Float,
    public val baseline: Float,
    public val alignment: Int,
    public val shift: Float,
    public val truncation: Boolean = false,
    public val label: String? = null,
)

internal class WhiskerInlineSpan(internal val attachment: WhiskerInlineAttachment, private val density: Float) : ReplacementSpan() {
    var alignedBaseline: Float? = null
    var baseline: Float = attachment.baseline * density
        private set

    override fun getSize(paint: Paint, text: CharSequence, start: Int, end: Int, metrics: Paint.FontMetricsInt?): Int {
        val font = paint.fontMetrics
        val height = attachment.height * density
        baseline = alignedBaseline ?: when (attachment.alignment) {
            1 -> -font.ascent
            2 -> {
                val bounds = Rect()
                paint.getTextBounds("x", 0, 1, bounds)
                (height - bounds.top) / 2
            }
            3 -> height - font.descent
            4 -> (attachment.baseline + attachment.shift) * density
            else -> attachment.baseline * density
        }
        metrics?.apply {
            ascent = min(ascent, floor(-baseline).toInt())
            descent = maxOf(descent, ceil(height - baseline).toInt())
            top = min(top, ascent)
            bottom = maxOf(bottom, descent)
        }
        return ceil(attachment.width * density).toInt()
    }

    override fun draw(canvas: Canvas, text: CharSequence, start: Int, end: Int, x: Float, top: Int, y: Int, bottom: Int, paint: Paint) = Unit
}

public fun WhiskerParagraph.inlinePlacements(layout: Layout, density: Float): WhiskerValue = WhiskerValue.Array(
    attachments.map { attachment ->
        val start = if (attachment.truncation) visibleEnd ?: source.length else attachment.start
        val end = start + 1
        val inside = start < layout.text.length && (!attachment.truncation || visibleEnd != null) && (attachment.truncation || visibleEnd == null || start < visibleEnd)
        val line = layout.getLineForOffset(start.coerceAtMost(layout.text.length))
        val ellipsisStart = layout.getLineStart(line) + layout.getEllipsisStart(line)
        val ellipsisEnd = ellipsisStart + layout.getEllipsisCount(line)
        val visible = inside && start < layout.getLineEnd(line) && !(start >= ellipsisStart && start < ellipsisEnd)
        val origin = if (visible) {
            val span = (layout.text as? Spanned)?.getSpans(start, end, WhiskerInlineSpan::class.java)?.singleOrNull()
            val left = layout.getPrimaryHorizontal(start) - if (layout.isRtlCharAt(start)) ceil(attachment.width * density) else 0f
            WhiskerValue.Array(listOf(
                WhiskerValue.Float((left / density).toDouble()),
                WhiskerValue.Float(((layout.getLineBaseline(line) - (span?.baseline ?: attachment.baseline * density)) / density).toDouble()),
            ))
        } else WhiskerValue.Null
        WhiskerValue.Map(mapOf("node" to WhiskerValue.Int(attachment.node), "origin" to origin))
    },
)
