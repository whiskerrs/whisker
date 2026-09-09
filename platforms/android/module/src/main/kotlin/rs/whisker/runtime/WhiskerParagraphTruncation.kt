package rs.whisker.runtime

import android.icu.text.BreakIterator
import android.text.StaticLayout

public fun WhiskerParagraph.truncateToFit(
    fits: (StaticLayout) -> Boolean,
    layout: (WhiskerParagraph) -> StaticLayout,
): Pair<StaticLayout, WhiskerParagraph> {
    val boundaries = mutableListOf<Int>()
    val iterator = BreakIterator.getCharacterInstance().apply { setText(source) }
    var boundary = iterator.first()
    while (boundary != BreakIterator.DONE) { boundaries += boundary; boundary = iterator.next() }
    var bestParagraph = withVisibleEnd(0)
    var best = layout(bestParagraph)
    var low = 0
    var high = boundaries.lastIndex
    var probes = 0
    while (low < high && probes++ < 60) {
        val middle = low + (high - low) / 2
        val candidateParagraph = withVisibleEnd(boundaries[middle])
        val candidate = layout(candidateParagraph)
        if (fits(candidate)) { best = candidate; bestParagraph = candidateParagraph; low = middle + 1 }
        else { high = middle }
    }
    return best to bestParagraph
}
