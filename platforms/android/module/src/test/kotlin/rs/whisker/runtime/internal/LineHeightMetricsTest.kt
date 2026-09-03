package rs.whisker.runtime.internal

import org.junit.Assert.assertEquals
import org.junit.Test

class LineHeightMetricsTest {
    @Test
    fun distributesPositiveLeadingAroundTheGlyphBox() {
        assertEquals(-21, centeredLineAscent(ascent = -15, descent = 5, targetHeight = 32))
        assertEquals(11, centeredLineDescent(ascent = -15, descent = 5, targetHeight = 32))
    }

    @Test
    fun contractsBothSidesForAFontHeightLargerThanLineHeight() {
        assertEquals(-11, centeredLineAscent(ascent = -15, descent = 5, targetHeight = 12))
        assertEquals(1, centeredLineDescent(ascent = -15, descent = 5, targetHeight = 12))
    }
}
