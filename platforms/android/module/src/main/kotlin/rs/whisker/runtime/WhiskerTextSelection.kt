package rs.whisker.runtime

import android.graphics.Path
import android.graphics.Rect
import android.graphics.Region
import android.graphics.RegionIterator
import android.text.Selection
import android.text.Spannable

internal fun WhiskerTextView.setWhiskerSelection(value: WhiskerValue) {
    val fields = (value as? WhiskerValue.Map)?.value ?: return
    if (!matchesRevision(fields)) return
    val range = textRange(fields) ?: return
    val content = text as? Spannable ?: return
    Selection.setSelection(content, displayOffset(range.first), displayOffset(range.second))
}

internal fun WhiskerTextView.clearWhiskerSelection(value: WhiskerValue) {
    val fields = (value as? WhiskerValue.Map)?.value ?: return
    if (matchesRevision(fields)) (text as? Spannable)?.let(Selection::removeSelection)
}

internal fun WhiskerTextView.queryWhiskerText(value: WhiskerValue) {
    val fields = (value as? WhiskerValue.Map)?.value ?: return
    val id = (fields["id"] as? WhiskerValue.Int)?.value ?: return
    val result = mutableMapOf<String, WhiskerValue>("id" to WhiskerValue.Int(id))
    if (!matchesRevision(fields)) result["error"] = WhiskerValue.Str("stale-layout")
    else when (fields["kind"]?.asString()) {
        "selectedText" -> result["text"] = WhiskerValue.Str(selectedLogicalText())
        "boundingRects" -> {
            val range = textRange(fields)
            if (range == null || paragraphLayout == null) result["error"] = WhiskerValue.Str("invalid-range")
            else result["rects"] = WhiskerValue.Array(selectionRects(displayOffset(range.first), displayOffset(range.second)))
        }
        else -> result["error"] = WhiskerValue.Str("unknown-query")
    }
    textEventSink?.invoke("textqueryresult", WhiskerValue.Map(result))
}

internal fun WhiskerTextView.selectedLogicalText(): String {
    val start = logicalOffset(minOf(selectionStart, selectionEnd))
    val end = logicalOffset(maxOf(selectionStart, selectionEnd))
    if (start < 0 || end > logicalText.length || start >= end) return ""
    return paragraph?.copyText(start, end) ?: logicalText.substring(start, end)
}

private fun WhiskerTextView.matchesRevision(fields: Map<String, WhiskerValue>): Boolean =
    (fields["revision"] as? WhiskerValue.Int)?.value?.let { it > 0 && it == preparedContent } == true

private fun WhiskerTextView.textRange(fields: Map<String, WhiskerValue>): Pair<Int, Int>? {
    val start = (fields["start"] as? WhiskerValue.Int)?.value ?: return null
    val end = (fields["end"] as? WhiskerValue.Int)?.value ?: return null
    if (start < 0 || start > end || end > logicalText.length) return null
    for (offset in listOf(start.toInt(), end.toInt())) {
        if (offset > 0 && offset < logicalText.length && Character.isHighSurrogate(logicalText[offset - 1]) && Character.isLowSurrogate(logicalText[offset])) return null
    }
    if (start == end) return start.toInt() to end.toInt()
    val boundaries = android.icu.text.BreakIterator.getCharacterInstance(java.util.Locale.ROOT)
    boundaries.setText(logicalText)
    return (if (boundaries.isBoundary(start.toInt())) start.toInt() else boundaries.preceding(start.toInt())) to
        (if (boundaries.isBoundary(end.toInt())) end.toInt() else boundaries.following(end.toInt()))
}

internal fun WhiskerTextView.logicalOffset(displayOffset: Int): Int {
    if (displayOffset < 0 || text.length == logicalText.length) return displayOffset
    var source = 0
    var display = 0
    while (display < displayOffset && source < logicalText.length) {
        if (text[display] == logicalText[source]) source++
        display++
    }
    return source
}

private fun WhiskerTextView.displayOffset(logicalOffset: Int): Int {
    paragraph?.visibleEnd?.let { return logicalOffset.coerceAtMost(it) }
    if (text.length == logicalText.length) return logicalOffset
    var source = 0
    var display = 0
    while (display < text.length && source < logicalOffset) {
        if (text[display] == logicalText[source]) source++
        display++
    }
    return display
}

private fun WhiskerTextView.selectionRects(start: Int, end: Int): List<WhiskerValue> {
    val layout = paragraphLayout ?: return emptyList()
    val density = resources.displayMetrics.density
    val result = mutableListOf<WhiskerValue>()
    for (line in 0 until layout.lineCount) {
        val from = maxOf(start, layout.getLineStart(line))
        val visibleEnd = if (layout.getEllipsisCount(line) > 0) layout.getLineStart(line) + layout.getEllipsisStart(line) else layout.getLineEnd(line)
        val to = minOf(end, visibleEnd)
        if (from >= to) continue
        val path = Path()
        layout.getSelectionPath(from, to, path)
        val region = Region()
        region.setPath(path, Region(0, layout.getLineTop(line), layout.width, layout.getLineBottom(line)))
        val iterator = RegionIterator(region)
        val rect = Rect()
        while (iterator.next(rect)) result += WhiskerValue.Map(mapOf(
            "x" to WhiskerValue.Float((rect.left / density).toDouble()),
            "y" to WhiskerValue.Float((rect.top / density).toDouble()),
            "width" to WhiskerValue.Float((rect.width() / density).toDouble()),
            "height" to WhiskerValue.Float((rect.height() / density).toDouble()),
        ))
    }
    return result
}
