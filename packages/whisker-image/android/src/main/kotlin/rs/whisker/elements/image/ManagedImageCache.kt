package rs.whisker.elements.image

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.graphics.Matrix
import android.media.ExifInterface
import android.os.Build
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors
import org.json.JSONObject
import kotlin.math.floor
import kotlin.math.min

internal object ManagedImageCache {
    val executor = Executors.newSingleThreadExecutor()
    private var limit = 64L * 1024 * 1024
    private var used = 0L
    val retainedBytes: Long get() = used
    private val entries = LinkedHashMap<String, Bitmap>(16, 0.75f, true)

    private fun hash(url: String): String = MessageDigest.getInstance("SHA-256")
        .digest(url.toByteArray(Charsets.UTF_8)).joinToString("") { "%02x".format(it) }

    private fun root(context: Context) = File(context.cacheDir, "tmp/whisker-images")
    private fun cost(bitmap: Bitmap) = bitmap.allocationByteCount.toLong()

    fun configure(value: String) {
        val bytes = value.toLong()
        require(bytes >= 0) { "invalid image cache budget" }
        limit = bytes
        trim(0)
    }

    private fun trim(incoming: Long) {
        val iterator = entries.entries.iterator()
        while (used > limit - incoming && iterator.hasNext()) {
            used -= cost(iterator.next().value)
            iterator.remove()
        }
    }

    fun remove(context: Context, url: String) {
        val digest = hash(url)
        val iterator = entries.entries.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            if (entry.key.startsWith("$digest:")) {
                used -= cost(entry.value)
                iterator.remove()
            }
        }
        val file = File(root(context), digest)
        check(!file.exists() || file.delete()) { "could not delete cached image" }
    }

    fun load(context: Context, url: String, headers: Map<String, String>, json: String): Bitmap {
        val options = JSONObject(json)
        fun bound(name: String): Int? {
            if (options.isNull(name)) return null
            val value = options.getLong(name)
            require(value in 1..Int.MAX_VALUE.toLong()) { "decode bounds must be positive 32-bit pixel sizes" }
            return value.toInt()
        }
        val width = bound("decode_width")
        val height = bound("decode_height")
        require(URL(url).protocol.lowercase() in listOf("http", "https")) { "managed image cache requires an HTTP(S) URL" }
        val digest = hash(url)
        val key = "$digest:${width ?: 0}:${height ?: 0}"
        entries[key]?.let { return it }
        val directory = root(context)
        val file = File(directory, digest)
        var bitmap: Bitmap? = if (file.exists()) runCatching { decode(file, width, height) }.getOrNull() else null
        if (bitmap == null) {
            if (file.exists()) check(file.delete()) { "could not delete corrupt image" }
            check(directory.isDirectory || directory.mkdirs()) { "could not create image cache directory" }
            val partial = File.createTempFile(digest, ".part", directory)
            try {
                download(url, headers, partial)
                bitmap = decode(partial, width, height)
                check(partial.renameTo(file)) { "could not commit cached image" }
            } finally { partial.delete() }
        }
        val result = requireNotNull(bitmap)
        val bytes = cost(result)
        if (bytes <= limit) {
            trim(bytes)
            entries[key] = result
            used += bytes
        }
        return result
    }

    private fun download(url: String, headers: Map<String, String>, target: File) {
        val connection = URL(url).openConnection() as HttpURLConnection
        try {
            connection.connectTimeout = 60_000
            connection.readTimeout = 60_000
            connection.useCaches = false
            headers.forEach { (name, value) -> connection.setRequestProperty(name, value) }
            check(connection.responseCode in 200..299) { "image HTTP ${connection.responseCode}" }
            connection.inputStream.use { input ->
                target.outputStream().use { output ->
                    val buffer = ByteArray(16 * 1024)
                    var total = 0L
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        total += count
                        check(total <= 64L * 1024 * 1024) { "image exceeds 64 MiB download limit" }
                        output.write(buffer, 0, count)
                    }
                }
            }
        } finally { connection.disconnect() }
    }

    private fun size(w: Int, h: Int, width: Int?, height: Int?): Pair<Int, Int> {
        require(w > 0 && h > 0) { "unsupported or corrupt image" }
        val scale = min(1.0, min((width ?: w).toDouble() / w, (height ?: h).toDouble() / h))
        return Pair(floor(w * scale).toInt().coerceAtLeast(1), floor(h * scale).toInt().coerceAtLeast(1))
    }

    private fun decode(file: File, width: Int?, height: Int?): Bitmap {
        if (Build.VERSION.SDK_INT >= 28) {
            return ImageDecoder.decodeBitmap(ImageDecoder.createSource(file)) { decoder, info, _ ->
                val (w, h) = size(info.size.width, info.size.height, width, height)
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
                decoder.setTargetSize(w, h)
            }
        }
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        val orientation = runCatching { ExifInterface(file.path).getAttributeInt(ExifInterface.TAG_ORIENTATION, 1) }.getOrDefault(1)
        val rotated = orientation in 5..8
        val sourceWidth = if (rotated) bounds.outHeight else bounds.outWidth
        val sourceHeight = if (rotated) bounds.outWidth else bounds.outHeight
        val (targetWidth, targetHeight) = size(sourceWidth, sourceHeight, width, height)
        var sample = 1
        while (sourceWidth / (sample * 2) >= targetWidth && sourceHeight / (sample * 2) >= targetHeight && sample <= (1 shl 28)) sample *= 2
        val options = BitmapFactory.Options().apply { inSampleSize = sample; inPreferredConfig = Bitmap.Config.ARGB_8888 }
        var image = requireNotNull(BitmapFactory.decodeFile(file.path, options)) { "image decode failed" }
        val transform = Matrix()
        when (orientation) {
            2 -> transform.setScale(-1f, 1f)
            3 -> transform.setRotate(180f)
            4 -> transform.setScale(1f, -1f)
            5 -> { transform.setRotate(90f); transform.postScale(-1f, 1f) }
            6 -> transform.setRotate(90f)
            7 -> { transform.setRotate(-90f); transform.postScale(-1f, 1f) }
            8 -> transform.setRotate(-90f)
        }
        if (!transform.isIdentity) image = Bitmap.createBitmap(image, 0, 0, image.width, image.height, transform, true)
        return Bitmap.createScaledBitmap(image, targetWidth, targetHeight, true)
    }
}
