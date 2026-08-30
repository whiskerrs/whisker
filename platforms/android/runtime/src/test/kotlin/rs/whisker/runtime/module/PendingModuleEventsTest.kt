package rs.whisker.runtime.module

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import rs.whisker.runtime.WhiskerValue

class PendingModuleEventsTest {
    @Test
    fun coalescesWakeupsAndRejectsEventsFromAnEarlierMountEpoch() {
        val pending = PendingModuleEvents()
        assertTrue(pending.offer(PendingModuleEvent(1, "demo", "tick", WhiskerValue.Int(1))))
        assertFalse(pending.offer(PendingModuleEvent(1, "demo", "tick", WhiskerValue.Int(2))))
        assertEquals(listOf(WhiskerValue.Int(1), WhiskerValue.Int(2)), pending.drain(1).map { it.payload })

        assertTrue(pending.offer(PendingModuleEvent(1, "demo", "tick", WhiskerValue.Int(3))))
        assertTrue(pending.drain(2).isEmpty())
    }
}
