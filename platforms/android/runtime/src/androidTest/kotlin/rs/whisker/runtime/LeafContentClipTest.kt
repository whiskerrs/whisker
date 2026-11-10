package rs.whisker.runtime

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.view.View
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.BeforeClass
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.roundToInt
import rs.whisker.runtime.paint.HostRasterResourceStore
import rs.whisker.runtime.scene.HostScene
import rs.whisker.runtime.scene.HostSceneOperation
import rs.whisker.runtime.scene.OP_CREATE
import rs.whisker.runtime.scene.OP_LAYOUT
import rs.whisker.runtime.scene.OP_PAINT

private class SolidContentView(context: Context) : View(context) {
    init {
        setBackgroundColor(Color.RED)
    }
}

private class SolidContentModule : Module() {
    override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("SolidContent")
        View(SOLID_ELEMENT, SolidContentView::class.java) {}
    }
}

private const val SOLID_ELEMENT = "whisker.test/SolidContent"

@RunWith(AndroidJUnit4::class)
class LeafContentClipTest {
    companion object {
        @JvmStatic
        @BeforeClass
        fun registerModules() {
            WhiskerModuleKernel.install(SolidContentModule())
        }
    }

    @Test
    fun leafModuleContentIsClippedToRoundedBorderBox() {
        val bitmap = render(SOLID_ELEMENT, WhiskerChildPolicy.None, inset = 0f, radius = 20f)
        assertTransparent(bitmap, 1f, 1f)
        assertTransparent(bitmap, 39f, 39f)
        assertRed(bitmap, 20f, 20f)
        assertRed(bitmap, 20f, 1f)
    }

    @Test
    fun leafModuleContentUsesTheInnerContentRadius() {
        // A 4px inset on a 12px radius leaves an 8px content radius.
        val bitmap = render(SOLID_ELEMENT, WhiskerChildPolicy.None, inset = 4f, radius = 12f)
        assertTransparent(bitmap, 5f, 5f)
        assertRed(bitmap, 8f, 8f)
        assertRed(bitmap, 20f, 5f)
    }

    @Test
    fun elementWithChildrenKeepsVisibleOverflow() {
        val bitmap = render(SOLID_ELEMENT, WhiskerChildPolicy.Elements, inset = 0f, radius = 20f)
        assertRed(bitmap, 1f, 1f)
    }

    private fun render(
        element: String,
        childPolicy: WhiskerChildPolicy,
        inset: Float,
        radius: Float,
    ): Bitmap {
        lateinit var bitmap: Bitmap
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val context = ApplicationProvider.getApplicationContext<Context>()
            val root = WhiskerContainerView(context)
            val elements = WhiskerElementRegistry.newBindings()
            assertTrue(
                WhiskerElementRegistry.bind(
                    elements,
                    listOf(
                        WhiskerElementRegistration(
                            elementType = 1,
                            name = element,
                            childPolicy = childPolicy,
                            measurement = WhiskerMeasurement.None,
                        ),
                    ),
                ),
            )
            val scene = HostScene(
                root,
                context,
                { _, _, _ -> },
                { _, _, _ -> },
                { _ -> },
                HostRasterResourceStore(),
                elements,
            )
            assertEquals(0, scene.beginFrame(0, 1, 0, 1))
            assertTrue(scene.stage(operation(OP_CREATE, member = 1)))
            assertTrue(
                scene.stage(
                    operation(
                        OP_LAYOUT,
                        numbers = floatArrayOf(
                            0f, 0f, SIZE, SIZE,
                            inset, inset, SIZE - inset * 2, SIZE - inset * 2,
                        ),
                    ),
                ),
            )
            assertTrue(
                scene.stage(
                    operation(
                        OP_PAINT,
                        numbers = boxPaint(radius),
                        names = Array(5) { "transparent" },
                    ),
                ),
            )
            assertTrue(scene.commit())

            val density = context.resources.displayMetrics.density
            val pixels = (SIZE * density).roundToInt()
            root.measure(
                View.MeasureSpec.makeMeasureSpec(pixels, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(pixels, View.MeasureSpec.EXACTLY),
            )
            root.layout(0, 0, pixels, pixels)
            bitmap = Bitmap.createBitmap(pixels, pixels, Bitmap.Config.ARGB_8888)
            root.draw(Canvas(bitmap))
        }
        return bitmap
    }

    private fun boxPaint(radius: Float): FloatArray {
        val numbers = ArrayList<Float>(53)
        numbers += listOf(0f, 0f, 0f, 0f, 1f)
        repeat(4) { numbers += listOf(0f, 0f) }
        repeat(4) { numbers += listOf(0f, 0f, 0f, 0f, 1f) }
        repeat(8) { numbers += listOf(radius, 0f) }
        repeat(4) { numbers += 0f }
        check(numbers.size == 53)
        return numbers.toFloatArray()
    }

    private fun pixel(bitmap: Bitmap, x: Float, y: Float): Int {
        val density = ApplicationProvider.getApplicationContext<Context>()
            .resources.displayMetrics.density
        return bitmap.getPixel(
            (x * density).roundToInt().coerceIn(0, bitmap.width - 1),
            (y * density).roundToInt().coerceIn(0, bitmap.height - 1),
        )
    }

    private fun assertTransparent(bitmap: Bitmap, x: Float, y: Float) {
        val actual = pixel(bitmap, x, y)
        assertEquals("alpha at ($x, $y): ${Color.valueOf(actual)}", 0, Color.alpha(actual))
    }

    private fun assertRed(bitmap: Bitmap, x: Float, y: Float) {
        assertEquals("pixel at ($x, $y)", Color.RED, pixel(bitmap, x, y))
    }

    private fun operation(
        tag: Int,
        member: Int = 0,
        numbers: FloatArray? = null,
        names: Array<String>? = null,
    ): HostSceneOperation = HostSceneOperation(
        tag = tag,
        flags = 0,
        node = 1,
        parent = 0,
        child = 0,
        index = 0,
        member = member,
        integer = 0,
        scalar = 0f,
        wide = 0,
        numbers = numbers,
        text = null,
        names = names,
        value = null,
    )
}

private const val SIZE = 40f
