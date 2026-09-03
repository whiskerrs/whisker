package rs.whisker.runtime.internal

import android.graphics.Paint
import android.text.style.LineHeightSpan
import kotlin.math.roundToInt

/** Internal text metric span matching CSS half-leading line boxes. */
public class CenteredLineHeightSpan(lineHeightPixels: Float) : LineHeightSpan {
    private val targetHeight: Int = lineHeightPixels.roundToInt().coerceAtLeast(1)

    override fun chooseHeight(
        text: CharSequence?,
        start: Int,
        end: Int,
        spanstartv: Int,
        v: Int,
        fm: Paint.FontMetricsInt,
    ) {
        val ascent = centeredLineAscent(fm.ascent, fm.descent, targetHeight)
        val descent = centeredLineDescent(fm.ascent, fm.descent, targetHeight)
        fm.ascent = ascent
        fm.top = ascent
        fm.descent = descent
        fm.bottom = descent
        fm.leading = 0
    }
}
