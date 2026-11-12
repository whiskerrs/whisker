package rs.whisker.elements.image

import android.content.ContextWrapper
import android.graphics.Bitmap
import android.test.InstrumentationTestCase
import java.io.ByteArrayOutputStream
import java.io.File
import java.net.ServerSocket
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@Suppress("DEPRECATION")
class ManagedImageCacheTest : InstrumentationTestCase() {
    fun testSizedDecodeLruAndDiskReuse() {
        val root = File(instrumentation.targetContext.cacheDir, UUID.randomUUID().toString())
        val context = object : ContextWrapper(instrumentation.targetContext) {
            override fun getCacheDir(): File = root
        }
        val png = ByteArrayOutputStream().apply {
            Bitmap.createBitmap(8, 6, Bitmap.Config.ARGB_8888).compress(Bitmap.CompressFormat.PNG, 100, this)
        }.toByteArray()
        val server = ServerSocket(0)
        server.soTimeout = 15_000
        val requests = AtomicInteger()
        val worker = Thread {
            repeat(4) {
                server.accept().use { socket ->
                    val reader = socket.getInputStream().bufferedReader()
                    while (!reader.readLine().isNullOrEmpty()) { }
                    requests.incrementAndGet()
                    socket.getOutputStream().apply {
                        write("HTTP/1.1 200 OK\r\nContent-Length: ${png.size}\r\nConnection: close\r\n\r\n".toByteArray())
                        write(png)
                        flush()
                    }
                }
            }
        }
        worker.isDaemon = true
        worker.start()
        try {
            ManagedImageCache.executor.submit {
                val base = "http://127.0.0.1:${server.localPort}"
                val options = """{"decode_width":4,"decode_height":4}"""
                ManagedImageCache.configure("1048576")
                val a = ManagedImageCache.load(context, "$base/a", emptyMap(), options)
                assertEquals(4, a.width)
                assertEquals(3, a.height)
                val cost = a.allocationByteCount.toLong()
                ManagedImageCache.configure((cost * 2).toString())
                val b = ManagedImageCache.load(context, "$base/b", emptyMap(), options)
                assertSame(a, ManagedImageCache.load(context, "$base/a", emptyMap(), options))
                ManagedImageCache.load(context, "$base/c", emptyMap(), options)
                assertTrue(ManagedImageCache.retainedBytes <= cost * 2)
                assertSame(a, ManagedImageCache.load(context, "$base/a", emptyMap(), options))
                assertNotSame(b, ManagedImageCache.load(context, "$base/b", emptyMap(), options))
                assertEquals(3, requests.get())
                ManagedImageCache.configure("0")
                assertEquals(0L, ManagedImageCache.retainedBytes)
                val original = ManagedImageCache.load(context, "$base/a", emptyMap(), "{}")
                assertEquals(8, original.width)
                assertEquals(6, original.height)
                assertEquals(3, requests.get())
                ManagedImageCache.remove(context, "$base/a")
                ManagedImageCache.load(context, "$base/a", emptyMap(), options)
                assertEquals(4, requests.get())
                assertEquals(0L, ManagedImageCache.retainedBytes)
                ManagedImageCache.configure("67108864")
            }.get(30, TimeUnit.SECONDS)
        } finally {
            server.close()
            worker.join(1_000)
            root.deleteRecursively()
        }
    }
    fun testNativeViewUsesDecodeBoundsAndReportsErrors() {
        val context = instrumentation.targetContext
        val url = "https://example.com/" + UUID.randomUUID().toString()
        val digest = java.security.MessageDigest.getInstance("SHA-256").digest(url.toByteArray()).joinToString("") { "%02x".format(it) }
        val file = File(context.cacheDir, "tmp/whisker-images/$digest")
        file.parentFile!!.mkdirs()
        file.outputStream().use { Bitmap.createBitmap(8, 6, Bitmap.Config.ARGB_8888).compress(Bitmap.CompressFormat.PNG, 100, it) }
        val loaded = java.util.concurrent.CountDownLatch(1)
        val failed = java.util.concurrent.CountDownLatch(1)
        lateinit var view: WhiskerImageView
        instrumentation.runOnMainSync {
            view = WhiskerImageView(context)
            view.installWhiskerEventSink { name, _ ->
                if (name == "load") loaded.countDown()
                if (name == "error") failed.countDown()
            }
            view.setCache("""{"_src":"$url","decode_width":8}""")
            view.setCache("""{"_src":"$url","decode_width":4,"decode_height":4}""")
            view.setSrc("")
        }
        assertTrue(loaded.await(5, TimeUnit.SECONDS))
        assertEquals(1L, failed.count)
        instrumentation.runOnMainSync {
            val bitmap = (view.view().drawable as android.graphics.drawable.BitmapDrawable).bitmap
            assertEquals(4, bitmap.width)
            assertEquals(3, bitmap.height)
            view.setCache("""{"_src":"$url","decode_width":0}""")
        }
        assertTrue(failed.await(5, TimeUnit.SECONDS))
        instrumentation.runOnMainSync { assertNull(view.view().drawable) }
        ManagedImageCache.executor.submit { ManagedImageCache.remove(context, url) }.get(5, TimeUnit.SECONDS)
    }

}
