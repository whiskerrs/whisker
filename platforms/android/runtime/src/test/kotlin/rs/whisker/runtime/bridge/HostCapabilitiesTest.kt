package rs.whisker.runtime.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HostCapabilitiesTest {
    @Test
    fun `Android 11 rejects backdrop blur without losing other visual effects`() {
        val profile = AndroidHostCapabilities.forApiLevel(30)

        assertTrue(profile.native and MobileAbi.CAPABILITY_VISUAL_EFFECTS != 0L)
        assertFalse(profile.native and MobileAbi.CAPABILITY_BACKDROP_BLUR != 0L)
        assertEquals(0L, profile.emulated)
    }

    @Test
    fun `Android 12 advertises native backdrop blur`() {
        val profile = AndroidHostCapabilities.forApiLevel(31)

        assertTrue(profile.native and MobileAbi.CAPABILITY_BACKDROP_BLUR != 0L)
        assertEquals(
            listOf(
                MobileAbi.MOBILE_ABI_MAJOR.toLong(),
                MobileAbi.MOBILE_ABI_MINOR.toLong(),
                MobileAbi.FRAME_PROTOCOL_MAJOR.toLong(),
                MobileAbi.FRAME_PROTOCOL_MINOR.toLong(),
            ),
            profile.wireValues().take(4),
        )
    }
}
