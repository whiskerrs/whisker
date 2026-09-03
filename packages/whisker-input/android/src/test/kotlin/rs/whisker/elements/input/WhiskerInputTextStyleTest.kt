package rs.whisker.elements.input

import org.junit.Assert.assertEquals
import org.junit.Test

class WhiskerInputTextStyleTest {
    @Test
    fun convertsLogicalFontSizeToPhysicalPixels() {
        assertEquals(48f, inputTextSizePixels(logicalPixels = 16f, density = 3f), 0f)
    }
}
