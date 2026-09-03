package rs.whisker.runtime.measure

import org.junit.Assert.assertEquals
import org.junit.Test

class TextMeasurementWidthTest {
    @Test
    fun wrappedTextReportsUsedLineWidthInsteadOfConstraintWidth() {
        assertEquals(
            42f,
            measuredTextWidth(
                knownWidth = 0f,
                hasKnownWidth = false,
                availableWidth = 300f,
                hasDefiniteAvailableWidth = true,
                wraps = true,
                usedLineWidthPixels = 83.2f,
                density = 2f,
            ),
        )
    }
}
