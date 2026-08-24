package rs.whisker.runtime.resource

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock
import rs.whisker.runtime.paint.HostRasterResourceStore

/** Observable state of one exact resource generation. */
enum class HostResourceState {
    Loading,
    Ready,
    Failed,
    Released,
}

/** Generation-specific resource state retained outside scene transactions. */
data class HostResourceSnapshot(
    val resourceId: Long,
    val generation: Long,
    val state: HostResourceState,
    val width: Int = 0,
    val height: Int = 0,
)

internal sealed interface HostRasterSource {
    data class Bytes(val mediaType: String, val data: ByteArray) : HostRasterSource
    data class Url(val value: String) : HostRasterSource
}

/**
 * Android-owned raster acquisition and decode lifecycle.
 *
 * Network and decode work always runs off the UI thread. Completion publishes
 * into the paint store only when both ResourceId and generation remain current.
 */
internal class HostResourceService(
    private val rasterStore: HostRasterResourceStore,
    private val onEvent: (HostResourceSnapshot) -> Unit,
    private val executor: ExecutorService = newResourceExecutor(),
) {
    private data class Key(val resourceId: Long, val generation: Long)

    private val lock = ReentrantLock()
    private val changed = lock.newCondition()
    private val highestGeneration = HashMap<Long, Long>()
    private val snapshots = HashMap<Key, HostResourceSnapshot>()

    fun load(resourceId: Long, generation: Long, source: HostRasterSource): Boolean {
        if (resourceId == 0L || generation == 0L || !validSource(source)) return false
        val key = Key(resourceId, generation)
        lock.withLock {
            val highest = highestGeneration[resourceId]
            if (highest != null && java.lang.Long.compareUnsigned(generation, highest) <= 0) {
                return false
            }
            highestGeneration[resourceId] = generation
            snapshots[key] = HostResourceSnapshot(resourceId, generation, HostResourceState.Loading)
            changed.signalAll()
        }
        return try {
            executor.execute { acquire(key, source) }
            true
        } catch (_: RuntimeException) {
            completeFailure(key)
            false
        }
    }

    fun release(resourceId: Long, generation: Long): Boolean {
        if (resourceId == 0L || generation == 0L) return false
        val snapshot = lock.withLock {
            val key = Key(resourceId, generation)
            if (snapshots[key] == null) return false
            HostResourceSnapshot(resourceId, generation, HostResourceState.Released).also {
                snapshots[key] = it
                rasterStore.release(resourceId, generation)
                changed.signalAll()
            }
        }
        onEvent(snapshot)
        return true
    }

    fun snapshot(resourceId: Long, generation: Long): HostResourceSnapshot? =
        lock.withLock { snapshots[Key(resourceId, generation)] }

    fun awaitTerminal(
        resourceId: Long,
        generation: Long,
        timeoutMillis: Long,
    ): HostResourceSnapshot? {
        var remaining = TimeUnit.MILLISECONDS.toNanos(timeoutMillis.coerceAtLeast(0L))
        lock.lock()
        try {
            val key = Key(resourceId, generation)
            while (true) {
                val snapshot = snapshots[key] ?: return null
                if (snapshot.state != HostResourceState.Loading || remaining <= 0L) return snapshot
                remaining = changed.awaitNanos(remaining)
            }
        } finally {
            lock.unlock()
        }
    }

    private fun acquire(key: Key, source: HostRasterSource) {
        val bitmap = try {
            val encoded = when (source) {
                is HostRasterSource.Bytes -> source.data
                is HostRasterSource.Url -> acquireUrl(source.value)
            }
            decodeRaster(encoded)
        } catch (_: Exception) {
            null
        } catch (_: OutOfMemoryError) {
            null
        }
        if (bitmap == null) {
            completeFailure(key)
            return
        }
        val ready = lock.withLock {
            val current = snapshots[key]
            if (
                highestGeneration[key.resourceId] != key.generation ||
                current?.state != HostResourceState.Loading
            ) {
                null
            } else if (!rasterStore.register(key.resourceId, key.generation, bitmap)) {
                HostResourceSnapshot(key.resourceId, key.generation, HostResourceState.Failed).also {
                    snapshots[key] = it
                    changed.signalAll()
                }
            } else {
                HostResourceSnapshot(
                    key.resourceId,
                    key.generation,
                    HostResourceState.Ready,
                    bitmap.width,
                    bitmap.height,
                ).also {
                    snapshots[key] = it
                    changed.signalAll()
                }
            }
        }
        if (ready == null) {
            bitmap.recycle()
        } else {
            onEvent(ready)
        }
    }

    private fun completeFailure(key: Key) {
        val failed = lock.withLock {
            val current = snapshots[key]
            if (
                highestGeneration[key.resourceId] != key.generation ||
                current?.state != HostResourceState.Loading
            ) {
                null
            } else {
                HostResourceSnapshot(key.resourceId, key.generation, HostResourceState.Failed).also {
                    snapshots[key] = it
                    changed.signalAll()
                }
            }
        }
        if (failed != null) onEvent(failed)
    }

    private fun acquireUrl(value: String): ByteArray {
        if (value.startsWith("data:", ignoreCase = true)) return decodeDataUrl(value)
        val url = URL(value)
        require(url.protocol == "http" || url.protocol == "https")
        val connection = url.openConnection() as HttpURLConnection
        return try {
            connection.connectTimeout = NETWORK_TIMEOUT_MILLIS
            connection.readTimeout = NETWORK_TIMEOUT_MILLIS
            connection.instanceFollowRedirects = true
            connection.requestMethod = "GET"
            connection.connect()
            require(connection.responseCode in 200..299)
            require(connection.contentLengthLong <= 0L || connection.contentLengthLong <= MAX_ENCODED_BYTES)
            connection.inputStream.use(::readBounded)
        } finally {
            connection.disconnect()
        }
    }

    private fun decodeDataUrl(value: String): ByteArray {
        val comma = value.indexOf(',')
        require(comma > 5)
        val metadata = value.substring(5, comma)
        require(metadata.substringBefore(';').startsWith("image/", ignoreCase = true))
        require(metadata.split(';').drop(1).any { it.equals("base64", ignoreCase = true) })
        return Base64.decode(value.substring(comma + 1), Base64.DEFAULT).also {
            require(it.isNotEmpty() && it.size <= MAX_ENCODED_BYTES)
        }
    }

    private fun decodeRaster(encoded: ByteArray): Bitmap? {
        if (encoded.isEmpty() || encoded.size > MAX_ENCODED_BYTES) return null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(encoded, 0, encoded.size, bounds)
        val width = bounds.outWidth
        val height = bounds.outHeight
        if (
            width <= 0 || height <= 0 || width > MAX_RASTER_AXIS || height > MAX_RASTER_AXIS ||
            width.toLong() * height.toLong() > MAX_RASTER_PIXELS
        ) {
            return null
        }
        return BitmapFactory.decodeByteArray(encoded, 0, encoded.size)
    }

    private fun readBounded(input: java.io.InputStream): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        while (true) {
            val count = input.read(buffer)
            if (count < 0) break
            require(output.size() + count <= MAX_ENCODED_BYTES)
            output.write(buffer, 0, count)
        }
        return output.toByteArray()
    }

    private fun validSource(source: HostRasterSource): Boolean = when (source) {
        is HostRasterSource.Bytes ->
            source.mediaType.startsWith("image/", ignoreCase = true) &&
                source.data.isNotEmpty() && source.data.size <= MAX_ENCODED_BYTES
        is HostRasterSource.Url -> source.value.isNotBlank()
    }

    private companion object {
        const val NETWORK_TIMEOUT_MILLIS = 15_000
        const val MAX_ENCODED_BYTES = 64 * 1024 * 1024
        const val MAX_RASTER_AXIS = 16_384
        const val MAX_RASTER_PIXELS = 100_000_000L

        private val threadNumber = AtomicInteger()

        private fun newResourceExecutor(): ExecutorService =
            Executors.newFixedThreadPool(2) { runnable ->
                Thread(runnable, "whisker-resource-${threadNumber.incrementAndGet()}").apply {
                    isDaemon = true
                }
            }
    }
}
