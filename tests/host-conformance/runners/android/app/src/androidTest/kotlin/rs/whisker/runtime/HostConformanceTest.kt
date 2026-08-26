package rs.whisker.runtime

import android.graphics.Bitmap
import android.graphics.Canvas
import android.util.Base64
import android.view.View
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.BeforeClass
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.abs
import kotlin.math.roundToInt
import rs.whisker.runtime.resource.HostResourceFailureCode
import rs.whisker.runtime.resource.HostResourceSnapshot
import rs.whisker.runtime.resource.HostResourceState

private const val BACKGROUND_PACKED_LAYERS = 256

@RunWith(AndroidJUnit4::class)
class HostConformanceTest {
    companion object {
        @JvmStatic
        @BeforeClass
        fun registerBuiltIns() {
            BuiltInElementModule().registerWithWhisker()
        }
    }

    @Test
    fun everySharedPaintScenarioUsesTheProductionAndroidView() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val manifest = JSONObject(asset("manifest.json"))
                var count = 0
                manifest.getJSONArray("cases").objects().forEach { entry ->
                    val scenario = JSONObject(asset(entry.getString("fixture")))
                    check(scenario.getInt("schema") == 1)
                    check(scenario.getString("id") == entry.getString("id"))
                    val testSide = scenario.getJSONObject("test")
                    if (testSide.getJSONArray("commands").objects().none {
                            it.getString("type") == "present_box" ||
                                it.getString("type") == "present_scene"
                        }) return@forEach
                    val id = scenario.getString("id")
                    val test = Driver(context, id).execute(testSide)
                    scenario.optJSONObject("reference")?.let { reference ->
                        val expected = Driver(context, id).execute(reference)
                        assertEquals(id, expected.width, test.width)
                        assertEquals(id, expected.height, test.height)
                        assertPixelsClose(id, test, expected)
                    }
                    count += 1
                }
                assertTrue("at least one shared paint scenario", count > 0)
            }
    }

    @Test
    fun rejectsAThreeDimensionalTransformInsteadOfFlatteningIt() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val matrix = floatArrayOf(
                    1f, 0f, 0f, 0f,
                    0f, 1f, 0f, 0f,
                    0f, 0f, 2f, 0f,
                    0f, 0f, 0f, 1f,
                )
                assertTrue(Driver(context, "android.transform-3d-rejection").rejectTransform(matrix))
            }
    }

    @Test
    fun rejectsInvalidOpacityValues() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                listOf(Float.NaN, Float.NEGATIVE_INFINITY, -0.1f, 1.1f).forEach { opacity ->
                    assertTrue(
                        Driver(context, "android.opacity-rejection").rejectOpacity(opacity),
                    )
                }
            }
    }

    @Test
    fun rejectsAnUnregisteredBackgroundResourceTransactionally() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(
                    Driver(context, "android.resource-rejection")
                        .rejectUnregisteredRasterResource(42L),
                )
            }
    }

    @Test
    fun preservesAllResourceIdBitsAcrossTheAndroidProjection() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(
                    Driver(context, "android.resource-id-bits")
                        .acceptRasterResource(-2L),
                )
            }
    }

    @Test
    fun reportsRasterDecodeFailure() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(Driver(context, "android.resource-failure").reportsRasterDecodeFailure())
            }
    }

    @Test
    fun typedResourceCommandsCopyBytesAndEmitOwnedEvents() {
        val instrumentation = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation()
        val ready = CountDownLatch(1)
        val received = AtomicReference<HostResourceSnapshot>()
        lateinit var view: WhiskerView
        val fixture = JSONObject(asset("core/resource-raster-lifecycle.json"))
        val encoded = fixture.getJSONObject("test")
            .getJSONArray("commands")
            .objects()
            .first { it.getString("type") == "load_raster_resource" }
            .getJSONObject("source")
            .getString("base64")
        val borrowedBytes = Base64.decode(encoded, Base64.DEFAULT)

        instrumentation.runOnMainSync {
            val context = ApplicationProvider.getApplicationContext<android.content.Context>()
            view = WhiskerView(context)
            view.observeRasterResourceEvents { event ->
                received.set(event)
                ready.countDown()
            }
            assertTrue(
                view.resourceCommandFromNative(
                    command = 1,
                    kind = 1,
                    source = 3,
                    resourceId = 91L,
                    generation = 1L,
                    identifier = "image/png",
                    data = borrowedBytes,
                ),
            )
            borrowedBytes.fill(0)
        }

        assertTrue("typed Ready event", ready.await(5, TimeUnit.SECONDS))
        val event = received.get()
        assertEquals(HostResourceState.Ready, event.state)
        assertEquals(91L, event.resourceId)
        assertEquals(1L, event.generation)
        assertEquals(2, event.width)
        assertEquals(2, event.height)
        assertEquals(HostResourceFailureCode.None, event.failureCode)

        val failed = CountDownLatch(1)
        instrumentation.runOnMainSync {
            view.observeRasterResourceEvents { unsupported ->
                received.set(unsupported)
                failed.countDown()
            }
            assertTrue(
                view.resourceCommandFromNative(
                    command = 1,
                    kind = 2,
                    source = 1,
                    resourceId = 92L,
                    generation = 1L,
                    identifier = "https://example.invalid/vector.svg",
                    data = byteArrayOf(),
                ),
            )
        }
        assertTrue("typed Failed event", failed.await(5, TimeUnit.SECONDS))
        val unsupported = received.get()
        assertEquals(HostResourceState.Failed, unsupported.state)
        assertEquals(92L, unsupported.resourceId)
        assertEquals(HostResourceFailureCode.Unsupported, unsupported.failureCode)
    }

    private fun asset(path: String): String =
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .context
            .assets
            .open(path)
            .bufferedReader()
            .use { it.readText() }
}

private class Driver(
    private val context: android.content.Context,
    private val id: String,
) {
    private val view = WhiskerView(context)
    private var logicalWidth = 0f
    private var logicalHeight = 0f
    private var checkpoint: Bitmap? = null

    init {
        view.beginBootstrapFromNative()
        view.registerElementFromNative(
            1,
            WhiskerBuiltInElements.VIEW,
            WhiskerChildPolicy.Elements.ordinal,
            WhiskerMeasurement.None.ordinal,
            intArrayOf(),
            intArrayOf(),
            emptyArray(),
            intArrayOf(),
            intArrayOf(),
            emptyArray(),
            intArrayOf(),
            intArrayOf(),
            emptyArray(),
        )
        check(view.finishBootstrapFromNative())
    }

    fun execute(side: JSONObject): Bitmap {
        side.getJSONArray("commands").objects().forEach { command ->
            when (command.getString("type")) {
                "attach_surface" -> {
                    logicalWidth = command.getDouble("width").toFloat()
                    logicalHeight = command.getDouble("height").toFloat()
                }
                "register_raster_resource" -> registerRasterResource(command)
                "load_raster_resource" -> loadRasterResource(command)
                "release_raster_resource" -> releaseRasterResource(command)
                "checkpoint_resource" -> checkpointRasterResource(command)
                "present_box" -> present(command)
                "present_scene" -> presentScene(command)
                "checkpoint" -> {
                    check(
                        command.getString("name") == "paint.box" ||
                            command.getString("name") == "paint.background-layers.linear-gradient" ||
                            command.getString("name") == "paint.background-layers.radial-gradient" ||
                            command.getString("name") == "paint.background-layers.conic-gradient" ||
                            command.getString("name") ==
                            "paint.background-layers.explicit-size-no-repeat" ||
                            command.getString("name") ==
                            "paint.background-layers.position-length-percentage" ||
                            command.getString("name") ==
                            "paint.background-layers.origin-border-box" ||
                            command.getString("name") ==
                            "paint.background-layers.clip-padding-box" ||
                            command.getString("name") == "paint.background-layers.repeat-x" ||
                            command.getString("name") == "paint.background-layers.repeat-y" ||
                            command.getString("name") == "paint.background-layers.repeat-space" ||
                            command.getString("name") ==
                            "paint.background-layers.repeat-space-single" ||
                            command.getString("name") == "paint.background-layers.repeat-round-x" ||
                            command.getString("name") == "paint.background-layers.repeat-round-y" ||
                            command.getString("name") ==
                            "paint.background-layers.repeat-round-position" ||
                            command.getString("name") ==
                            "paint.background-layers.origin-content-box" ||
                            command.getString("name") ==
                            "paint.background-layers.clip-content-box" ||
                            command.getString("name") ==
                            "paint.background-layers.clip-border-area" ||
                            command.getString("name") ==
                            "paint.background-layers.stacking" ||
                            command.getString("name") ==
                            "paint.background-layers.resource-image" ||
                            command.getString("name") ==
                            "paint.background-layers.resource-lifecycle" ||
                            command.getString("name") ==
                            "paint.background-layers.intrinsic-auto" ||
                            command.getString("name") ==
                            "paint.background-layers.size-contain" ||
                            command.getString("name") ==
                            "paint.background-layers.size-cover" ||
                            command.getString("name") ==
                            "paint.background-layers.round-auto-aspect-ratio" ||
                            command.getString("name") ==
                            "paint.visual-effects.box-shadow-offset" ||
                            command.getString("name") ==
                            "paint.visual-effects.box-shadow-spread" ||
                            command.getString("name") ==
                            "paint.visual-effects.box-shadow-blur",
                    )
                    checkpoint = capture()
                    command.optJSONArray("samples")?.let { samples ->
                        assertPixelSamples(
                            id,
                            checkNotNull(checkpoint),
                            samples,
                            context.resources.displayMetrics.density,
                        )
                    }
                    command.optJSONArray("relations")?.let { relations ->
                        assertPixelRelations(
                            id,
                            checkNotNull(checkpoint),
                            relations,
                            context.resources.displayMetrics.density,
                        )
                    }
                }
                else -> error("unsupported Android paint command: ${command.getString("type")}")
            }
        }
        return checkNotNull(checkpoint)
    }

    fun rejectTransform(transform: FloatArray): Boolean {
        check(view.beginFrameFromNative(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        check(stage(tag = 9, numbers = transform))
        return !view.commitFrameFromNative()
    }

    fun rejectOpacity(opacity: Float): Boolean {
        check(view.beginFrameFromNative(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        check(stage(tag = 10, scalar = opacity))
        return !view.commitFrameFromNative()
    }

    fun rejectUnregisteredRasterResource(resourceId: Long): Boolean {
        return !commitRasterResource(resourceId)
    }

    fun acceptRasterResource(resourceId: Long): Boolean {
        val bitmap = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
        check(view.registerRasterResourceFromNative(resourceId, bitmap))
        return commitRasterResource(resourceId)
    }

    fun reportsRasterDecodeFailure(): Boolean {
        check(
            view.loadRasterResourceBytesFromNative(
                7L,
                1L,
                "image/png",
                byteArrayOf(0, 1, 2, 3),
            ),
        )
        return view.awaitRasterResourceFromNative(7L, 1L, 5_000)?.state ==
            HostResourceState.Failed
    }

    private fun commitRasterResource(resourceId: Long): Boolean {
        check(view.beginFrameFromNative(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        val layer = backgroundGeometry(null).apply { appendResourceId(resourceId, this) }
        val packed = ArrayList<Float>().apply {
            add(1f)
            add(3f)
            add(0f)
            add(layer.size.toFloat())
            addAll(layer)
        }
        check(
            stage(
                tag = 21,
                flags = BACKGROUND_PACKED_LAYERS,
                numbers = packed.toFloatArray(),
                names = emptyArray(),
            ),
        )
        return view.commitFrameFromNative()
    }

    private fun registerRasterResource(command: JSONObject) {
        val width = command.getInt("width")
        val height = command.getInt("height")
        val pixels = command.getJSONArray("pixels")
        check(pixels.length() == width * height)
        val colors = IntArray(pixels.length()) { index -> fixtureColor(pixels.getJSONObject(index)) }
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888).apply {
            setPixels(colors, 0, width, 0, 0, width, height)
        }
        check(view.registerRasterResourceFromNative(command.getLong("id"), bitmap))
    }

    private fun loadRasterResource(command: JSONObject) {
        val resourceId = command.getLong("id")
        val generation = command.getLong("generation")
        val source = command.getJSONObject("source")
        val accepted = when (source.getString("kind")) {
            "bytes" -> view.loadRasterResourceBytesFromNative(
                resourceId,
                generation,
                source.getString("media_type"),
                Base64.decode(source.getString("base64"), Base64.DEFAULT),
            )
            "url" -> view.loadRasterResourceUrlFromNative(
                resourceId,
                generation,
                source.getString("value"),
            )
            else -> error("unsupported raster resource source: $source")
        }
        check(accepted)
    }

    private fun releaseRasterResource(command: JSONObject) {
        check(
            view.releaseRasterResourceFromNative(
                command.getLong("id"),
                command.getLong("generation"),
            ),
        )
    }

    private fun checkpointRasterResource(command: JSONObject) {
        val snapshot = checkNotNull(
            view.awaitRasterResourceFromNative(
                command.getLong("id"),
                command.getLong("generation"),
                5_000,
            ),
        )
        val expectedState = when (command.getString("state")) {
            "ready" -> HostResourceState.Ready
            "failed" -> HostResourceState.Failed
            "released" -> HostResourceState.Released
            else -> error("unsupported resource checkpoint: $command")
        }
        check(snapshot.state == expectedState)
        if (expectedState == HostResourceState.Ready) {
            check(snapshot.width == command.getInt("width"))
            check(snapshot.height == command.getInt("height"))
        }
    }

    private fun present(command: JSONObject) {
        val revision = command.getLong("revision")
        check(view.beginFrameFromNative(0, 1, 0, revision) == 0)
        check(stage(tag = 1, member = 1))
        val rect = command.getJSONArray("rect").floats()
        check(stage(tag = 6, numbers = rect + floatArrayOf(0f, 0f, 0f, 0f)))
        val (numbers, names) = paint(command)
        check(stage(tag = 7, numbers = numbers, names = names))
        check(view.commitFrameFromNative())
    }

    private fun presentScene(command: JSONObject) {
        val revision = command.getLong("revision")
        val nodes = command.getJSONArray("nodes")
        check(view.beginFrameFromNative(0, 1, 0, revision) == 0)
        nodes.objects().forEach { node ->
            check(stage(tag = 1, node = node.getLong("id"), member = 1))
        }
        val childIndices = HashMap<Long, Int>()
        nodes.objects().forEach { node ->
            if (!node.isNull("parent")) {
                val parent = node.getLong("parent")
                val index = childIndices.getOrDefault(parent, 0)
                check(
                    stage(
                        tag = 3,
                        node = 0,
                        parent = parent,
                        child = node.getLong("id"),
                        index = index,
                    ),
                )
                childIndices[parent] = index + 1
            }
        }
        nodes.objects().forEach { node ->
            val id = node.getLong("id")
            val rect = node.getJSONArray("rect").floats()
            val content = node.optJSONArray("content_box")?.floats()
                ?: floatArrayOf(0f, 0f, rect[2], rect[3])
            check(
                stage(
                    tag = 6,
                    node = id,
                    numbers = rect + content,
                ),
            )
            val (numbers, names) = paint(node)
            check(stage(tag = 7, node = id, numbers = numbers, names = names))
            val clip = node.optJSONObject("clip") ?: JSONObject(
                "{\"horizontal\":\"visible\",\"vertical\":\"visible\"}",
            )
            val flags =
                (if (clip.getString("horizontal") == "hidden") 1 else 0) or
                    (if (clip.getString("vertical") == "hidden") 2 else 0)
            check(stage(tag = 8, node = id, flags = flags))
            node.optJSONArray("transform")?.let { transform ->
                check(stage(tag = 9, node = id, numbers = transform.floats()))
            }
            if (node.has("opacity")) {
                check(stage(tag = 10, node = id, scalar = node.getDouble("opacity").toFloat()))
            }
            node.optString("visibility").takeIf(String::isNotEmpty)?.let { visibility ->
                check(stage(tag = 11, node = id, integer = if (visibility == "visible") 1 else 0))
            }
            if (node.has("z_order")) {
                check(stage(tag = 12, node = id, integer = node.getInt("z_order")))
            }
            node.optJSONObject("linear_gradient")?.let { gradient ->
                val (numbers, names) = linearGradient(
                    node.optJSONObject("background_layer"),
                    gradient,
                )
                check(
                    stage(
                        tag = 21,
                        flags = 0,
                        node = id,
                        scalar = gradient.getDouble("angle_degrees").toFloat(),
                        numbers = numbers,
                        names = names,
                    ),
                )
            }
            node.optJSONObject("radial_gradient")?.let { gradient ->
                val (numbers, names) = radialGradient(
                    node.optJSONObject("background_layer"),
                    gradient,
                )
                check(
                    stage(
                        tag = 21,
                        flags = 1,
                        node = id,
                        numbers = numbers,
                        names = names,
                    ),
                )
            }
            node.optJSONObject("conic_gradient")?.let { gradient ->
                val (numbers, names) = conicGradient(
                    node.optJSONObject("background_layer"),
                    gradient,
                )
                check(
                    stage(
                        tag = 21,
                        flags = 2,
                        node = id,
                        scalar = gradient.getDouble("from_degrees").toFloat(),
                        numbers = numbers,
                        names = names,
                    ),
                )
            }
            node.optJSONArray("background_layers")?.let { layers ->
                val numbers = ArrayList<Float>()
                val names = ArrayList<String>()
                numbers += layers.length().toFloat()
                layers.objects().forEach { layer ->
                    val geometry = layer.optJSONObject("geometry")
                    val image = layer.getJSONObject("image")
                    val (kind, scalar, payload) = when {
                        image.has("resource") -> {
                            val numbers = backgroundGeometry(geometry)
                            appendResourceId(image.getLong("resource"), numbers)
                            Triple(3, 0f, numbers.toFloatArray() to emptyArray<String>())
                        }
                        image.has("linear_gradient") -> {
                            val gradient = image.getJSONObject("linear_gradient")
                            Triple(
                                0,
                                gradient.getDouble("angle_degrees").toFloat(),
                                linearGradient(geometry, gradient),
                            )
                        }
                        image.has("radial_gradient") -> Triple(
                            1,
                            0f,
                            radialGradient(geometry, image.getJSONObject("radial_gradient")),
                        )
                        image.has("conic_gradient") -> {
                            val gradient = image.getJSONObject("conic_gradient")
                            Triple(
                                2,
                                gradient.getDouble("from_degrees").toFloat(),
                                conicGradient(geometry, gradient),
                            )
                        }
                        else -> error("unsupported background image: $image")
                    }
                    numbers += kind.toFloat()
                    numbers += scalar
                    numbers += payload.first.size.toFloat()
                    payload.first.forEach(numbers::add)
                    payload.second.forEach(names::add)
                }
                check(
                    stage(
                        tag = 21,
                        flags = BACKGROUND_PACKED_LAYERS,
                        node = id,
                        numbers = numbers.toFloatArray(),
                        names = names.toTypedArray(),
                    ),
                )
            }
            node.optJSONArray("box_shadows")?.let { shadows ->
                val shadowNumbers = ArrayList<Float>(shadows.length() * 10)
                val shadowNames = ArrayList<String>(shadows.length())
                shadows.objects().forEach { shadow ->
                    val offset = shadow.getJSONArray("offset")
                    shadowNumbers += offset.getDouble(0).toFloat()
                    shadowNumbers += offset.getDouble(1).toFloat()
                    shadowNumbers += shadow.getDouble("blur_radius").toFloat()
                    shadowNumbers += shadow.getDouble("spread_radius").toFloat()
                    shadowNumbers += if (shadow.optBoolean("inset", false)) 1f else 0f
                    appendColor(shadow.getJSONObject("color"), shadowNumbers, shadowNames)
                }
                check(
                    stage(
                        tag = 22,
                        node = id,
                        numbers = shadowNumbers.toFloatArray(),
                        names = shadowNames.toTypedArray(),
                    ),
                )
            }
        }
        check(view.commitFrameFromNative())
    }

    private fun stage(
        tag: Int,
        flags: Int = 0,
        node: Long = 1,
        parent: Long = 0,
        child: Long = 0,
        index: Int = 0,
        member: Int = 0,
        integer: Int = 0,
        scalar: Float = 0f,
        numbers: FloatArray? = null,
        names: Array<String>? = null,
    ): Boolean = view.stageOperationFromNative(
        tag,
        flags,
        node,
        parent,
        child,
        index,
        member,
        integer,
        scalar,
        0,
        numbers,
        null,
        names,
        null,
    )

    private fun paint(command: JSONObject): Pair<FloatArray, Array<String>> {
        val numbers = ArrayList<Float>(53)
        val names = ArrayList<String>(5)
        appendColor(command.getJSONObject("background"), numbers, names)
        val border = command.optJSONObject("border")
        val widths = border?.getJSONArray("widths")?.floats() ?: FloatArray(4)
        widths.forEach { width -> numbers += listOf(width, 0f) }
        val colors = border?.getJSONArray("colors")
        repeat(4) { index ->
            appendColor(
                colors?.getJSONObject(index) ?: JSONObject("{\"kind\":\"named\",\"value\":\"transparent\"}"),
                numbers,
                names,
            )
        }
        val radii = border?.getJSONArray("radii")
        val radiiHorizontal = FloatArray(4)
        val radiiVertical = FloatArray(4)
        repeat(4) { index ->
            when (val radius = radii?.get(index)) {
                is Number -> {
                    radiiHorizontal[index] = radius.toFloat()
                    radiiVertical[index] = radius.toFloat()
                }
                is JSONArray -> {
                    radiiHorizontal[index] = radius.getDouble(0).toFloat()
                    radiiVertical[index] = radius.getDouble(1).toFloat()
                }
                null -> Unit
                else -> error("unsupported border radius: $radius")
            }
        }
        radiiHorizontal.forEach { radius -> numbers += listOf(radius, 0f) }
        radiiVertical.forEach { radius -> numbers += listOf(radius, 0f) }
        val styles = border?.getJSONArray("styles")?.strings() ?: Array(4) { "none" }
        styles.forEach { style -> numbers += borderStyle(style).toFloat() }
        check(numbers.size == 53)
        check(names.size == 5)
        return numbers.toFloatArray() to names.toTypedArray()
    }

    private fun appendColor(color: JSONObject, numbers: MutableList<Float>, names: MutableList<String>) {
        if (color.getString("kind") == "named") {
            numbers += listOf(0f, 0f, 0f, 0f, 1f)
            names += color.getString("value")
        } else {
            numbers += listOf(
                1f,
                color.getDouble("red").toFloat(),
                color.getDouble("green").toFloat(),
                color.getDouble("blue").toFloat(),
                color.getDouble("alpha").toFloat(),
            )
            names += ""
        }
    }

    private fun linearGradient(
        geometry: JSONObject?,
        gradient: JSONObject,
    ): Pair<FloatArray, Array<String>> {
        check(!gradient.optBoolean("repeating", false))
        val numbers = backgroundGeometry(geometry)
        val names = ArrayList<String>()
        gradient.getJSONArray("stops").objects().forEach { stop ->
            appendColor(stop.getJSONObject("color"), numbers, names)
            numbers += 0f
            numbers += stop.getDouble("position").toFloat()
        }
        return numbers.toFloatArray() to names.toTypedArray()
    }

    private fun radialGradient(
        geometry: JSONObject?,
        gradient: JSONObject,
    ): Pair<FloatArray, Array<String>> {
        val numbers = backgroundGeometry(geometry)
        val names = ArrayList<String>()
        val center = gradient.getJSONArray("center")
        val radii = gradient.getJSONArray("radii")
        numbers += listOf(
            center.getDouble(0).toFloat(), 0f,
            center.getDouble(1).toFloat(), 0f,
            radii.getDouble(0).toFloat(), 0f,
            radii.getDouble(1).toFloat(), 0f,
        )
        gradient.getJSONArray("stops").objects().forEach { stop ->
            appendColor(stop.getJSONObject("color"), numbers, names)
            numbers += 0f
            numbers += stop.getDouble("position").toFloat()
        }
        return numbers.toFloatArray() to names.toTypedArray()
    }

    private fun conicGradient(
        geometry: JSONObject?,
        gradient: JSONObject,
    ): Pair<FloatArray, Array<String>> {
        val numbers = backgroundGeometry(geometry)
        val names = ArrayList<String>()
        val center = gradient.getJSONArray("center")
        numbers += listOf(
            center.getDouble(0).toFloat(), 0f,
            center.getDouble(1).toFloat(), 0f,
        )
        gradient.getJSONArray("stops").objects().forEach { stop ->
            appendColor(stop.getJSONObject("color"), numbers, names)
            numbers += 0f
            numbers += stop.getDouble("position").toFloat()
        }
        return numbers.toFloatArray() to names.toTypedArray()
    }

    private fun backgroundGeometry(geometry: JSONObject?): ArrayList<Float> {
        val position = geometry?.optJSONArray("position")
        val numbers = ArrayList<Float>()
        appendLengthPercentage(position?.optJSONObject(0), numbers)
        appendLengthPercentage(position?.optJSONObject(1), numbers)
        val (sizeKind, sizeWidth, sizeHeight) = backgroundSize(geometry?.opt("size"))
        appendLengthPercentage(sizeWidth, numbers)
        appendLengthPercentage(sizeHeight, numbers)
        numbers += sizeKind.toFloat()
        numbers += backgroundRepeat(geometry?.optString("repeat_x", "repeat") ?: "repeat").toFloat()
        numbers += backgroundRepeat(geometry?.optString("repeat_y", "repeat") ?: "repeat").toFloat()
        numbers += backgroundBox(geometry?.optString("origin", "padding") ?: "padding").toFloat()
        numbers += backgroundBox(geometry?.optString("clip", "border") ?: "border").toFloat()
        numbers += 0f // scroll attachment
        numbers += 0f // normal blend mode
        check(numbers.size == 15)
        return numbers
    }

    private fun backgroundSize(value: Any?): Triple<Int, JSONObject?, JSONObject?> = when (value) {
        null, JSONObject.NULL -> Triple(0, null, null)
        is String -> when (value) {
            "auto" -> Triple(0, null, null)
            "cover" -> Triple(2, null, null)
            "contain" -> Triple(3, null, null)
            else -> error("unsupported background size: $value")
        }
        is JSONArray -> {
            check(value.length() == 2)
            Triple(1, value.getJSONObject(0), value.getJSONObject(1))
        }
        is JSONObject -> {
            val width = value.opt("width").takeUnless { it == null || it == JSONObject.NULL }
                as? JSONObject
            val height = value.opt("height").takeUnless { it == null || it == JSONObject.NULL }
                as? JSONObject
            when {
                width != null && height != null -> Triple(1, width, height)
                width != null -> Triple(4, width, null)
                height != null -> Triple(5, null, height)
                else -> Triple(0, null, null)
            }
        }
        else -> error("unsupported background size: $value")
    }

    private fun appendLengthPercentage(value: JSONObject?, numbers: MutableList<Float>) {
        numbers += value?.optDouble("length", 0.0)?.toFloat() ?: 0f
        numbers += value?.optDouble("fraction", 0.0)?.toFloat() ?: 0f
    }

    private fun appendResourceId(resourceId: Long, numbers: MutableList<Float>) {
        check(resourceId != 0L)
        repeat(4) { wordIndex ->
            numbers += ((resourceId ushr (wordIndex * 16)) and 0xffffL).toFloat()
        }
    }

    private fun backgroundRepeat(value: String): Int = when (value) {
        "repeat" -> 0
        "no_repeat" -> 1
        "space" -> 2
        "round" -> 3
        else -> error("unsupported background repeat: $value")
    }

    private fun backgroundBox(value: String): Int = when (value) {
        "border" -> 0
        "padding" -> 1
        "content" -> 2
        "border_area" -> 3
        else -> error("unsupported background box: $value")
    }

    private fun borderStyle(value: String): Int {
        val names = arrayOf(
            "none", "hidden", "solid", "dashed", "dotted",
            "double", "groove", "ridge", "inset", "outset",
        )
        return names.indexOf(value).also { check(it >= 0) { "unknown border style: $value" } }
    }

    private fun capture(): Bitmap {
        val density = context.resources.displayMetrics.density
        val width = (logicalWidth * density).roundToInt().coerceAtLeast(1)
        val height = (logicalHeight * density).roundToInt().coerceAtLeast(1)
        view.measure(
            View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY),
        )
        view.layout(0, 0, width, height)
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(bitmap))
        return bitmap
    }
}

private fun JSONArray.objects(): Sequence<JSONObject> =
    (0 until length()).asSequence().map(::getJSONObject)

private fun JSONArray.floats(): FloatArray =
    FloatArray(length()) { index -> getDouble(index).toFloat() }

private fun JSONArray.strings(): Array<String> =
    Array(length()) { index -> getString(index) }

private fun assertPixelSamples(id: String, bitmap: Bitmap, samples: JSONArray, density: Float) {
    samples.objects().forEach { sample ->
        val point = sample.getJSONArray("point")
        val x = (point.getDouble(0) * density).roundToInt()
        val y = (point.getDouble(1) * density).roundToInt()
        check(x in 0 until bitmap.width && y in 0 until bitmap.height)
        val actual = bitmap.getPixel(x, y)
        val expected = fixtureColor(sample.getJSONObject("color"))
        val tolerance = sample.optInt("tolerance", 0)
        val difference = listOf(
            android.graphics.Color.alpha(actual) to android.graphics.Color.alpha(expected),
            android.graphics.Color.red(actual) to android.graphics.Color.red(expected),
            android.graphics.Color.green(actual) to android.graphics.Color.green(expected),
            android.graphics.Color.blue(actual) to android.graphics.Color.blue(expected),
        ).maxOf { (left, right) -> abs(left - right) }
        assertTrue(
            "$id sample ($x, $y) differs by $difference: " +
                "actual=${android.graphics.Color.valueOf(actual)} " +
                "expected=${android.graphics.Color.valueOf(expected)}",
            difference <= tolerance,
        )
    }
}

private fun assertPixelRelations(id: String, bitmap: Bitmap, relations: JSONArray, density: Float) {
    relations.objects().forEach { relation ->
        val first = pixelAt(bitmap, relation.getJSONArray("first"), density)
        val second = pixelAt(bitmap, relation.getJSONArray("second"), density)
        val firstLuminance = luminance(first)
        val secondLuminance = luminance(second)
        val minimum = relation.optInt("minimum_difference", 0)
        val matches = when (val kind = relation.getString("relation")) {
            "lighter" -> firstLuminance >= secondLuminance + minimum
            "darker" -> firstLuminance + minimum <= secondLuminance
            else -> error("unknown pixel relation: $kind")
        }
        assertTrue(
            "$id ${relation.getString("relation")}: " +
                "$first ($firstLuminance) vs $second ($secondLuminance)",
            matches,
        )
    }
}

private fun pixelAt(bitmap: Bitmap, point: JSONArray, density: Float): Int {
    val x = (point.getDouble(0) * density).roundToInt()
    val y = (point.getDouble(1) * density).roundToInt()
    check(x in 0 until bitmap.width && y in 0 until bitmap.height)
    return bitmap.getPixel(x, y)
}

private fun luminance(color: Int): Int =
    (android.graphics.Color.red(color) * 299 +
        android.graphics.Color.green(color) * 587 +
        android.graphics.Color.blue(color) * 114) / 1000

private fun fixtureColor(value: JSONObject): Int =
    if (value.getString("kind") == "named") {
        val name = value.getString("value")
        when (name) {
            "transparent" -> android.graphics.Color.TRANSPARENT
            "green" -> android.graphics.Color.rgb(0, 128, 0)
            else -> android.graphics.Color.parseColor(name)
        }
    } else {
        android.graphics.Color.argb(
            (value.getDouble("alpha") * 255.0).roundToInt(),
            value.getInt("red"),
            value.getInt("green"),
            value.getInt("blue"),
        )
    }

private fun assertPixelsClose(id: String, actual: Bitmap, expected: Bitmap) {
    val actualPixels = IntArray(actual.width * actual.height)
    val expectedPixels = IntArray(expected.width * expected.height)
    actual.getPixels(actualPixels, 0, actual.width, 0, 0, actual.width, actual.height)
    expected.getPixels(expectedPixels, 0, expected.width, 0, 0, expected.width, expected.height)
    var largestDifference = 0
    actualPixels.zip(expectedPixels).forEach { (left, right) ->
        repeat(4) { shift ->
            largestDifference = maxOf(
                largestDifference,
                abs(
                    ((left ushr (shift * 8)) and 0xff) -
                        ((right ushr (shift * 8)) and 0xff),
                ),
            )
        }
    }
    assertTrue("$id pixel difference $largestDifference", largestDifference <= 1)
}
