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
    @Test
    fun preservesDragTransitionsAndReleaseVelocityWhileSummingMovement() {
        val pending = PendingContinuousEvents()
        fun sample(drag: Boolean, delta: Double, velocity: Double = 0.0) = PendingElementEvent(
            7L, "scroll", WhiskerValue.Map(mapOf(
                "isDragging" to WhiskerValue.Bool(drag),
                "deltaY" to WhiskerValue.Float(delta),
                "velocityY" to WhiskerValue.Float(velocity),
            )), 0.0,
        )
        pending.offer(sample(true, 0.0))
        pending.offer(sample(true, 10.0))
        pending.offer(sample(true, 20.0))
        pending.offer(sample(false, 0.0, 800.0))
        pending.offer(sample(false, 5.0))
        pending.offer(sample(false, 10.0))
        val result = pending.drain()
        assertEquals(listOf(sample(true, 30.0), sample(false, 0.0, 800.0), sample(false, 15.0)), result)
    }

    @Test
    fun preservesZeroVelocityReleaseAndCancellation() {
        val pending = PendingContinuousEvents()
        fun sample(drag: Boolean, cancelled: Boolean = false) = PendingElementEvent(
            7L, "scroll", WhiskerValue.Map(mapOf(
                "isDragging" to WhiskerValue.Bool(drag),
                "isDragCancelled" to WhiskerValue.Bool(cancelled),
            )), 0.0,
        )
        val samples = listOf(sample(true), sample(false), sample(true), sample(false, true), sample(false))
        samples.forEach(pending::offer)
        assertEquals(samples, pending.drain())
        pending.offer(sample(true))
        pending.clear()
        assertEquals(emptyList<PendingElementEvent>(), pending.drain())
    }
}
