package rs.whisker.runtime

import android.graphics.Bitmap
import android.graphics.Canvas
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
import kotlin.math.abs
import kotlin.math.roundToInt

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
                "present_box" -> present(command)
                "present_scene" -> presentScene(command)
                "checkpoint" -> {
                    check(
                        command.getString("name") == "paint.box" ||
                            command.getString("name") == "paint.background-layers.linear-gradient" ||
                            command.getString("name") == "paint.background-layers.radial-gradient",
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
            check(
                stage(
                    tag = 6,
                    node = id,
                    numbers = rect + floatArrayOf(0f, 0f, rect[2], rect[3]),
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
                val (numbers, names) = linearGradient(gradient)
                check(
                    stage(
                        tag = 21,
                        flags = if (gradient.optBoolean("repeating", false)) 1 else 0,
                        node = id,
                        scalar = gradient.getDouble("angle_degrees").toFloat(),
                        numbers = numbers,
                        names = names,
                    ),
                )
            }
            node.optJSONObject("radial_gradient")?.let { gradient ->
                val (numbers, names) = radialGradient(gradient)
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

    private fun linearGradient(gradient: JSONObject): Pair<FloatArray, Array<String>> {
        val numbers = ArrayList<Float>()
        val names = ArrayList<String>()
        gradient.getJSONArray("stops").objects().forEach { stop ->
            appendColor(stop.getJSONObject("color"), numbers, names)
            numbers += 0f
            numbers += stop.getDouble("position").toFloat()
        }
        return numbers.toFloatArray() to names.toTypedArray()
    }

    private fun radialGradient(gradient: JSONObject): Pair<FloatArray, Array<String>> {
        val numbers = ArrayList<Float>()
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
        val x = (point.getDouble(0) * density).toInt()
        val y = (point.getDouble(1) * density).toInt()
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
    val x = (point.getDouble(0) * density).toInt()
    val y = (point.getDouble(1) * density).toInt()
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
