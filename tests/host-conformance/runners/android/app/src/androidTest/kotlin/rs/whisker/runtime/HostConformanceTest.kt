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
    fun everySharedPaintReftestUsesTheProductionAndroidView() {
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
                    val reference = scenario.optJSONObject("reference") ?: return@forEach
                    val test = Driver(context).execute(scenario.getJSONObject("test"))
                    val expected = Driver(context).execute(reference)
                    assertEquals(scenario.getString("id"), expected.width, test.width)
                    assertEquals(scenario.getString("id"), expected.height, test.height)
                    assertPixelsClose(scenario.getString("id"), test, expected)
                    count += 1
                }
                assertTrue("at least one shared paint reftest", count > 0)
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

private class Driver(private val context: android.content.Context) {
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
                "checkpoint" -> {
                    check(command.getString("name") == "paint.box")
                    checkpoint = capture()
                }
                else -> error("unsupported Android paint command: ${command.getString("type")}")
            }
        }
        return checkNotNull(checkpoint)
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

    private fun stage(
        tag: Int,
        member: Int = 0,
        numbers: FloatArray? = null,
        names: Array<String>? = null,
    ): Boolean = view.stageOperationFromNative(
        tag,
        0,
        1,
        0,
        0,
        0,
        member,
        0,
        0f,
        0,
        numbers,
        null,
        names,
        null,
    )

    private fun paint(command: JSONObject): Pair<FloatArray, Array<String>> {
        val numbers = ArrayList<Float>(41)
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
        val radii = border?.getJSONArray("radii")?.floats() ?: FloatArray(4)
        radii.forEach { radius -> numbers += listOf(radius, 0f) }
        val styles = border?.getJSONArray("styles")?.strings() ?: Array(4) { "none" }
        styles.forEach { style -> numbers += borderStyle(style).toFloat() }
        check(numbers.size == 45)
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
