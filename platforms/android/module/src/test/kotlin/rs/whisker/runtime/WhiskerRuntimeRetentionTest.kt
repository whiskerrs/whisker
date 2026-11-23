package rs.whisker.runtime

import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class WhiskerRuntimeRetentionTest {
    @Before
    fun setUp() = WhiskerRuntimeRetention.resetForTesting()

    @After
    fun tearDown() = WhiskerRuntimeRetention.resetForTesting()

    @Test
    fun leasesAreCountedAndOnlyTheLastReleaseNotifies() {
        var released = 0
        WhiskerRuntimeRetention.addOnReleasedListener { released += 1 }
        assertFalse(WhiskerRuntimeRetention.isRequested)

        val playback = WhiskerRuntimeRetention.acquire("playback")
        val download = WhiskerRuntimeRetention.acquire("download")
        assertTrue(WhiskerRuntimeRetention.isRequested)
        assertEquals("playback", playback.reason)

        playback.release()
        assertTrue(WhiskerRuntimeRetention.isRequested)
        assertEquals(0, released)

        download.release()
        assertFalse(WhiskerRuntimeRetention.isRequested)
        assertEquals(1, released)
    }

    @Test
    fun releasingALeaseTwiceDoesNotReleaseAnotherLease() {
        var released = 0
        WhiskerRuntimeRetention.addOnReleasedListener { released += 1 }
        val first = WhiskerRuntimeRetention.acquire("first")
        val second = WhiskerRuntimeRetention.acquire("second")

        first.release()
        first.release()
        assertTrue(WhiskerRuntimeRetention.isRequested)
        assertEquals(0, released)

        second.release()
        assertFalse(WhiskerRuntimeRetention.isRequested)
        assertEquals(1, released)
    }

    @Test
    fun removedListenersAreNotNotified() {
        var released = 0
        val listener = RuntimeRetentionReleasedListener { released += 1 }
        WhiskerRuntimeRetention.addOnReleasedListener(listener)
        WhiskerRuntimeRetention.addOnReleasedListener(listener)
        WhiskerRuntimeRetention.acquire("once").release()
        assertEquals(1, released)

        WhiskerRuntimeRetention.removeOnReleasedListener(listener)
        WhiskerRuntimeRetention.acquire("again").release()
        assertEquals(1, released)
    }
}
