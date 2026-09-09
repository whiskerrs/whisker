package rs.whisker.runtime

import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Region
import android.graphics.RegionIterator
import android.text.Layout
import android.text.TextPaint

internal fun WhiskerParagraph.drawBackgrounds(canvas: Canvas, layout: Layout, density: Float) {
    val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    for (run in runs) {
        val style = run.paint ?: continue
        if (Color.alpha(style.background) == 0) continue
        paint.color = style.background
        fragments(layout, run.start, run.end) { _, rect ->
            val radii = FloatArray(8)
            style.radii.forEachIndexed { index, radius ->
                radii[index * 2] = radius[0] * density + radius[1] * rect.width()
                radii[index * 2 + 1] = radius[2] * density + radius[3] * rect.height()
            }
            val factor = minOf(1f,
                radiusScale(rect.width(), radii[0] + radii[2]),
                radiusScale(rect.width(), radii[6] + radii[4]),
                radiusScale(rect.height(), radii[1] + radii[7]),
                radiusScale(rect.height(), radii[3] + radii[5]))
            radii.indices.forEach { radii[it] *= factor }
            val path = Path().apply { addRoundRect(RectF(rect), radii, Path.Direction.CW) }
            canvas.drawPath(path, paint)
        }
    }
}

internal fun WhiskerParagraph.drawDecorations(canvas: Canvas, layout: Layout, basePaint: TextPaint, density: Float) {
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    for (run in runs) {
        val style = run.paint ?: continue
        if (!style.underline && !style.strike) continue
        val font = TextPaint(basePaint)
        ParagraphSpan(run, density).updateMeasureState(font)
        val stroke = maxOf(density, font.textSize / 16f)
        paint.color = style.decorationColor
        paint.strokeWidth = stroke
        fragments(layout, run.start, run.end) { line, rect ->
            val baseline = layout.getLineBaseline(line).toFloat() + font.baselineShift
            if (style.underline) drawWhiskerTextDecoration(canvas, paint, style.decorationStyle, rect.left.toFloat(), rect.right.toFloat(), baseline + stroke * 1.5f, stroke)
            if (style.strike) drawWhiskerTextDecoration(canvas, paint, style.decorationStyle, rect.left.toFloat(), rect.right.toFloat(), baseline + font.fontMetrics.ascent * 0.35f, stroke)
        }
    }
}

private fun radiusScale(extent: Int, sum: Float): Float = if (sum > 0) extent / sum else 1f

private fun fragments(layout: Layout, start: Int, end: Int, consume: (Int, Rect) -> Unit) {
    for (line in 0 until layout.lineCount) {
        val lineStart = layout.getLineStart(line)
        val visibleEnd = if (layout.getEllipsisCount(line) > 0) lineStart + layout.getEllipsisStart(line) else layout.getLineEnd(line)
        val first = maxOf(start, lineStart)
        val last = minOf(end, visibleEnd)
        if (first >= last) continue
        val path = Path()
        layout.getSelectionPath(first, last, path)
        val region = Region()
        region.setPath(path, Region(0, layout.getLineTop(line), layout.width, layout.getLineBottom(line)))
        val iterator = RegionIterator(region)
        val rect = Rect()
        while (iterator.next(rect)) consume(line, rect)
    }
}
