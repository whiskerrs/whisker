package rs.whisker.runtime

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PendingContinuousEventsTest {
    @Test
    fun coalescesScrollSamplesPerNodeUntilTheyAreDrained() {
        val events = PendingContinuousEvents()

        events.offer(PendingElementEvent(7L, "scroll", WhiskerValue.Int(1), 10.0))
        events.offer(PendingElementEvent(7L, "scroll", WhiskerValue.Int(2), 11.0))
        events.offer(PendingElementEvent(9L, "scroll", WhiskerValue.Int(3), 12.0))

        assertEquals(
            listOf(
                PendingElementEvent(7L, "scroll", WhiskerValue.Int(2), 11.0),
                PendingElementEvent(9L, "scroll", WhiskerValue.Int(3), 12.0),
            ),
            events.drain(),
        )
        assertEquals(emptyList<PendingElementEvent>(), events.drain())
    }
}
