package rs.whisker.runtime

import android.graphics.Path
import android.graphics.Rect
import android.graphics.Region
import android.graphics.RegionIterator
import android.text.Layout

public fun WhiskerParagraph.measurementGeometry(layout: Layout, density: Float): WhiskerValue {
    val fragments = mutableListOf<WhiskerValue>()
    val sources = sourceRanges.ifEmpty { listOf(0 until layout.text.length) }
    val lines = (0 until layout.lineCount).map { line ->
        val start = layout.getLineStart(line)
        val ellipsisStart = start + layout.getEllipsisStart(line)
        val truncated = layout.getEllipsisCount(line) > 0
        val visibleEnd = minOf(this.visibleEnd ?: layout.text.length, if (truncated) ellipsisStart else layout.getLineEnd(line))
        val customTruncated = this.visibleEnd != null && line == layout.lineCount - 1
        val end = if (truncated || customTruncated) source.length else visibleEnd
        sources.forEach { source ->
            val rangeStart = maxOf(source.first, start)
            val rangeEnd = minOf(source.last + 1, visibleEnd)
            if (rangeStart < rangeEnd) {
                val path = Path()
                layout.getSelectionPath(rangeStart, rangeEnd, path)
                val region = Region()
                region.setPath(path, Region(0, layout.getLineTop(line), layout.width, layout.getLineBottom(line)))
                val iterator = RegionIterator(region)
                val rect = Rect()
                while (iterator.next(rect)) {
                    fragments += WhiskerValue.Map(mapOf(
                        "start" to WhiskerValue.Int(rangeStart.toLong()), "end" to WhiskerValue.Int(rangeEnd.toLong()),
                        "bounds" to paragraphRect(rect.left.toFloat(), rect.top.toFloat(), rect.width().toFloat(), rect.height().toFloat(), density),
                    ))
                }
            }
        }
        WhiskerValue.Map(mapOf(
            "start" to WhiskerValue.Int(start.toLong()), "end" to WhiskerValue.Int(end.toLong()),
            "ellipsis" to WhiskerValue.Int(if (truncated || customTruncated) (end - visibleEnd).toLong() else 0),
            "baseline" to WhiskerValue.Float((layout.getLineBaseline(line) / density).toDouble()),
            "bounds" to paragraphRect(layout.getLineLeft(line), layout.getLineTop(line).toFloat(), layout.getLineRight(line) - layout.getLineLeft(line), (layout.getLineBottom(line) - layout.getLineTop(line)).toFloat(), density),
        ))
    }
    return WhiskerValue.Map(mapOf(
        "placements" to inlinePlacements(layout, density), "lines" to WhiskerValue.Array(lines), "fragments" to WhiskerValue.Array(fragments),
    ))
}

private fun paragraphRect(x: Float, y: Float, width: Float, height: Float, density: Float): WhiskerValue =
    WhiskerValue.Array(listOf(x, y, width, height).map { WhiskerValue.Float((it / density).toDouble()) })
