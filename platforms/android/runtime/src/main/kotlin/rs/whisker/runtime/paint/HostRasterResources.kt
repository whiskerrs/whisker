package rs.whisker.runtime.paint

import android.graphics.Bitmap
import java.util.concurrent.ConcurrentHashMap

/** Decoded raster resources keyed by the lossless 64-bit protocol ResourceId bits. */
internal class HostRasterResourceStore {
    private val bitmaps = ConcurrentHashMap<Long, Bitmap>()

    fun register(resourceId: Long, bitmap: Bitmap): Boolean {
        if (resourceId == 0L || bitmap.isRecycled || bitmap.width <= 0 || bitmap.height <= 0) {
            return false
        }
        bitmaps[resourceId] = bitmap
        return true
    }

    fun resolve(resourceId: Long): Bitmap? = bitmaps[resourceId]?.takeUnless(Bitmap::isRecycled)
}
