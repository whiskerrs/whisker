package rs.whisker.elements.input

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InputLineBoxTest {
    @Test
    fun splitsHalfLeadingIntoPadding() {
        // 16px Roboto at density 2.625: ascent -39, descent 11, line-height 63.
        val box = InputLineBox(lineHeight = 63, ascent = -39, descent = 11)
        assertEquals(6, box.paddingTop)
        assertEquals(7, box.paddingBottom)
    }

    @Test
    fun linesSumToLineHeightTimesCount() {
        val box = InputLineBox(lineHeight = 63, ascent = -39, descent = 11)
        val inner = box.lineHeight
        val last = box.descent - box.ascent
        // Three lines: two inner lines, one last line, plus the padding.
        assertEquals(3 * 63, 2 * inner + last + box.paddingTop + box.paddingBottom)
    }

    @Test
    fun detectsLastLine() {
        assertTrue(isLastInputLine("", 0))
        assertTrue(isLastInputLine("abc", 3))
        assertTrue(isLastInputLine("a\nbc", 4))
        assertFalse(isLastInputLine("a\nbc", 2))
        // A trailing newline is followed by an empty line Android lays out
        // without spans, so the line ending in it is not the last one.
        assertFalse(isLastInputLine("abc\n", 4))
    }
}
