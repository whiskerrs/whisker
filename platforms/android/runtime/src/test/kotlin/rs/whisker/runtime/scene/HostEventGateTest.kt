package rs.whisker.runtime.scene

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HostEventGateTest {
    @Test
    fun elementEventsWaitUntilAfterFramePresentationReturns() {
        var scheduled: (() -> Unit)? = null
        val gate = HostEventGate { scheduled = it }
        val events = mutableListOf<String>()

        gate.beginFrame()
        gate.dispatch { events += "first" }
        gate.dispatch { events += "second" }
        gate.endFrame()

        assertTrue(events.isEmpty())
        scheduled?.invoke()
        assertEquals(listOf("first", "second"), events)
    }
}
