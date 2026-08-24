package rs.whisker.runtime.paint

import android.graphics.Bitmap
import java.util.concurrent.ConcurrentHashMap

/** Decoded raster resources keyed by the lossless 64-bit protocol ResourceId bits. */
internal class HostRasterResourceStore {
    private data class Entry(val generation: Long, val bitmap: Bitmap)

    private val bitmaps = ConcurrentHashMap<Long, Entry>()

    fun register(resourceId: Long, bitmap: Bitmap): Boolean =
        registerEntry(resourceId, 0L, bitmap)

    fun register(resourceId: Long, generation: Long, bitmap: Bitmap): Boolean {
        if (generation == 0L) return false
        return registerEntry(resourceId, generation, bitmap)
    }

    private fun registerEntry(resourceId: Long, generation: Long, bitmap: Bitmap): Boolean {
        if (
            resourceId == 0L || bitmap.isRecycled ||
            bitmap.width <= 0 || bitmap.height <= 0
        ) {
            return false
        }
        bitmaps[resourceId] = Entry(generation, bitmap)
        return true
    }

    fun release(resourceId: Long, generation: Long) {
        bitmaps.computeIfPresent(resourceId) { _, entry ->
            entry.takeUnless { it.generation == generation }
        }
    }

    fun resolve(resourceId: Long): Bitmap? =
        bitmaps[resourceId]?.bitmap?.takeUnless(Bitmap::isRecycled)
}
