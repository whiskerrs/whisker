package rs.whisker.runtime

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InitialFrameGateTest {
    @Test
    fun waitsForWindowInsetsBeforePresentingTheFirstFrame() {
        val gate = InitialFrameGate()
        val calls = mutableListOf<String>()

        repeat(3) {
            assertFalse(gate.run(false, { calls += "events" }, { calls += "frame" }))
        }
        assertTrue(calls.isEmpty())

        assertTrue(gate.run(true, { calls += "events" }, { calls += "frame" }))
        assertEquals(listOf("events", "frame"), calls)
    }

    @Test
    fun appliesQueuedInsetsBeforeTheFirstFrameAndEveryLaterFrame() {
        val gate = InitialFrameGate()
        var top = 0
        var pendingTop = 24
        val presented = mutableListOf<Int>()
        val flush = { top = pendingTop }
        val present = { presented.add(top); Unit }

        gate.run(true, flush, present)
        pendingTop = 48
        gate.run(true, flush, present)
        assertEquals(listOf(24, 48), presented)
    }

    @Test
    fun acceptsValidZeroInsetsWithoutWaitingForAPositiveValue() {
        val gate = InitialFrameGate()
        var top: Int? = null
        val presented = mutableListOf<Int?>()

        assertTrue(gate.run(true, { top = 0 }, { presented.add(top); Unit }))
        assertEquals(listOf(0), presented)
    }

    @Test
    fun aNewRuntimeWaitsForItsOwnWindowInsets() {
        val calls = mutableListOf<String>()
        InitialFrameGate().run(true, {}, { calls += "old" })

        val recreated = InitialFrameGate()
        assertFalse(recreated.run(false, {}, { calls += "premature" }))
        assertTrue(recreated.run(true, {}, { calls += "new" }))
        assertEquals(listOf("old", "new"), calls)
    }

    @Test
    fun anEstablishedRuntimeCanKeepDrivingWithoutANewInsetsSnapshot() {
        val gate = InitialFrameGate()
        var frames = 0
        gate.run(true, {}, { frames++ })
        assertTrue(gate.run(false, {}, { frames++ }))
        assertEquals(2, frames)
    }
}
