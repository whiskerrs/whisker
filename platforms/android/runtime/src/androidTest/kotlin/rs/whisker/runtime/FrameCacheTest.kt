package rs.whisker.runtime

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import rs.whisker.runtime.measure.FrameCache

@RunWith(AndroidJUnit4::class)
class FrameCacheTest {
    @Test
    fun oneFrameReusesEveryEntryItBuilt() {
        val cache = FrameCache<Int, Int>()
        var builds = 0
        repeat(2) { (0 until 1_000).forEach { key -> cache.getOrPut(key) { builds++; key } } }
        assertEquals(1_000, builds)
    }

    @Test
    fun entriesSurviveOnlyWhileThePreviousMeasuringFrameUsedThem() {
        val cache = FrameCache<String, Int>()
        val built = mutableListOf<String>()
        fun use(key: String) = cache.getOrPut(key) { built += key; built.size }
        use("a")
        use("b")
        cache.endFrame()
        use("a")
        cache.endFrame()
        cache.endFrame()
        use("a")
        use("b")
        assertEquals(listOf("a", "b", "b"), built)
    }
}
