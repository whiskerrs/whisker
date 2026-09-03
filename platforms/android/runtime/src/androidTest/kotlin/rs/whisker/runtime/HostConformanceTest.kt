package rs.whisker.runtime

import android.graphics.Bitmap
import android.os.Build
import android.view.ViewGroup
import android.widget.TextView
import android.graphics.Canvas
import android.view.InputDevice
import android.view.MotionEvent
import android.util.Base64
import android.text.Spanned
import android.text.TextDirectionHeuristics
import android.text.style.LeadingMarginSpan
import android.view.View
import android.view.Gravity
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
import rs.whisker.runtime.input.normalizePointerInput
import rs.whisker.runtime.bridge.MobileAbi
import rs.whisker.runtime.measure.HostMeasureBatchAbi
import rs.whisker.runtime.measure.resolveTextLayoutSemantics
import rs.whisker.runtime.paint.HostBoxShadow
import rs.whisker.runtime.paint.ResolvedBoxGeometry
import rs.whisker.runtime.paint.drawOuterBoxShadows
import rs.whisker.runtime.paint.HostRasterResourceStore
import rs.whisker.runtime.scene.HostAccessibility
import rs.whisker.runtime.scene.HostNode
import rs.whisker.runtime.scene.HostScene
import rs.whisker.runtime.scene.HostSceneOperation
import rs.whisker.runtime.scene.OP_BACKGROUND_LAYERS
import rs.whisker.runtime.scene.OP_CREATE
import rs.whisker.runtime.scene.OP_DELETE
import rs.whisker.runtime.scene.OP_INSERT
import rs.whisker.runtime.scene.OP_LAYOUT

private const val BACKGROUND_PACKED_LAYERS = 256

private data class CapturedPointerInput(
    val event: Int,
    val pointerId: Long,
    val kind: Int,
    val buttons: Int,
    val changedButton: Int,
    val timestampMs: Double,
    val x: Float,
    val y: Float,
)

private class FailingElementView(context: android.content.Context) : View(context)

private class FailingElementModule : Module() {
    override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("FailingElement")
        View("whisker.test/Failing", FailingElementView::class.java) {
            Prop("checked") { _: FailingElementView, _: WhiskerValue ->
                error("module property failure")
            }
        }
    }
}

@RunWith(AndroidJUnit4::class)
class HostConformanceTest {
    companion object {
        @JvmStatic
        @BeforeClass
        fun registerBuiltIns() {
            WhiskerModuleKernel.install(BuiltInElementModule())
        }
    }

    @Test
    fun pooledBuiltInVisibilityIsResetBeforeReuse() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val registration = WhiskerElementRegistration(
                    elementType = 1,
                    name = WhiskerBuiltInElements.TEXT,
                    childPolicy = WhiskerChildPolicy.PlainText,
                    measurement = WhiskerMeasurement.Text,
                )
                val elements = WhiskerElementRegistry.newBindings()
                assertTrue(WhiskerElementRegistry.bind(elements, listOf(registration)))
                val mounted = checkNotNull(
                    elements.mount(1, context) { _, _ -> },
                )
                mounted.view.visibility = View.INVISIBLE

                mounted.prepareForReuse { _, _ -> }

                assertEquals(View.VISIBLE, mounted.view.visibility)
            }
    }

    @Test
    fun throwingModulePropertyDisablesOnlyTheMountedElement() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                WhiskerModuleKernel.install(FailingElementModule())
                val registration = WhiskerElementRegistration(
                    elementType = 20,
                    name = "whisker.test/Failing",
                    childPolicy = WhiskerChildPolicy.None,
                    measurement = WhiskerMeasurement.None,
                    properties = listOf(
                        WhiskerPropertyBinding(1, "checked", WhiskerValueKind.Bool),
                    ),
                )
                val elements = WhiskerElementRegistry.newBindings()
                assertTrue(WhiskerElementRegistry.bind(elements, listOf(registration)))
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val mounted = checkNotNull(
                    elements.mount(20, context) { _, _ -> },
                )

                mounted.setProperty(1, WhiskerValue.Bool(true))
                mounted.setProperty(1, WhiskerValue.Bool(false))

                assertEquals(View.VISIBLE, mounted.view.visibility)
            }
    }

    @Test
    fun invalidStructureAndLayoutAreRejectedBeforeViewMutation() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val root = WhiskerContainerView(context)
                val elements = WhiskerElementRegistry.newBindings()
                assertTrue(
                    WhiskerElementRegistry.bind(
                        elements,
                        listOf(
                            WhiskerElementRegistration(
                                1,
                                WhiskerBuiltInElements.VIEW,
                                WhiskerChildPolicy.Elements,
                                WhiskerMeasurement.None,
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
                listOf(
                    hostOperation(OP_CREATE, node = 1, member = 1),
                    hostOperation(OP_CREATE, node = 2, member = 1),
                    hostOperation(OP_CREATE, node = 3, member = 1),
                    hostOperation(OP_INSERT, parent = 1, child = 2),
                    hostOperation(OP_INSERT, parent = 2, child = 3),
                ).forEach { assertTrue(scene.stage(it)) }
                assertTrue(scene.commit())
                val rootNode = root.getChildAt(0) as HostNode
                val childNode = (0 until rootNode.childCount)
                    .map(rootNode::getChildAt)
                    .first { it is HostNode }
                val childParent = childNode.parent

                assertEquals(0, scene.beginFrame(1, 1, 1, 2))
                scene.stage(hostOperation(OP_INSERT, parent = 3, child = 1))
                assertTrue(!scene.commit())
                assertTrue(childNode.parent === childParent)

                assertEquals(0, scene.beginFrame(1, 1, 1, 2))
                scene.stage(hostOperation(OP_DELETE, node = 2))
                scene.stage(hostOperation(OP_BACKGROUND_LAYERS, node = 3))
                assertTrue(!scene.commit())
                assertTrue(childNode.parent === childParent)

                assertEquals(0, scene.beginFrame(1, 1, 1, 2))
                scene.stage(
                    hostOperation(
                        OP_LAYOUT,
                        node = 1,
                        numbers = floatArrayOf(0f, 0f, Float.NaN, 10f, 0f, 0f, 10f, 10f),
                    ),
                )
                assertTrue(!scene.commit())
            }
    }

    @Test
    fun commonAccessibilityMapsToAndroidNodeSemantics() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val node = HostNode(context, WhiskerBuiltInElements.VIEW, null)
                node.setAccessibility(
                    HostAccessibility(
                        label = "Playback",
                        hint = "Starts the episode",
                        role = "button",
                        identifier = "playback-button",
                        hidden = false,
                        modal = false,
                        disabled = true,
                        selected = true,
                        checked = "mixed",
                        expanded = false,
                    ),
                )

                assertEquals("Playback", node.contentDescription)
                assertEquals(false, node.isEnabled)
                assertEquals(true, node.isSelected)
                val info = node.createAccessibilityNodeInfo()
                assertEquals("playback-button", info.viewIdResourceName)
                assertEquals(android.widget.Button::class.java.name, info.className)
                assertEquals(true, info.isCheckable)
                assertEquals(false, info.isChecked)
                if (Build.VERSION.SDK_INT >= 30) {
                    assertEquals("mixed", info.stateDescription)
                }
            }
    }

    @Test
    fun scrollViewEmitsLogicalScrollGeometry() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val scrollView = WhiskerScrollContainerView(context)
                val density = context.resources.displayMetrics.density
                scrollView.layout(0, 0, (100 * density).roundToInt(), (80 * density).roundToInt())
                scrollView.contentView.layout(
                    0,
                    0,
                    (100 * density).roundToInt(),
                    (300 * density).roundToInt(),
                )
                var detail: WhiskerValue? = null
                var presentedX = Float.NaN
                var presentedY = Float.NaN
                scrollView.installWhiskerPresentationSink { x, y ->
                    presentedX = x
                    presentedY = y
                }
                scrollView.installWhiskerEventSink { name, value ->
                    if (name == "scroll") detail = value
                }

                scrollView.scrollTo(0, (120 * density).roundToInt())

                val values = (detail as WhiskerValue.Map).value
                assertEquals(120.0, (values.getValue("scrollTop") as WhiskerValue.Float).value, 0.001)
                assertEquals(80.0, (values.getValue("viewportHeight") as WhiskerValue.Float).value, 0.001)
                assertEquals(300.0, (values.getValue("scrollHeight") as WhiskerValue.Float).value, 0.001)
                assertEquals(0f, presentedX, 0.001f)
                assertEquals(120f, presentedY, 0.001f)
            }
    }

    @Test
    fun backdropBlurMatchesTheAdvertisedAndroidVersionBoundary() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertEquals(
                    Build.VERSION.SDK_INT >= Build.VERSION_CODES.S,
                    Driver(context, "backdrop-version-boundary").acceptsBackdropBlur(16f),
                )
            }
    }

    @Test
    fun pointerCaptureOperationsReachTheAndroidSurface() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(Driver(context, "pointer-capture").acceptsPointerCapture())
            }
    }

    @Test
    fun zOrderUsesPhysicalSiblingOrderWithoutAndroidElevation() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(Driver(context, "z-order").verifyPhysicalZOrder())
            }
    }

    @Test
    fun layoutSizesRoundToTheNearestPhysicalPixel() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(Driver(context, "layout-rounding").verifyLayoutRounding())
            }
    }

    @Test
    fun pointerCaptureDisallowsInterceptionFromTheCapturedNodesParent() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val elements = WhiskerElementRegistry.newBindings()
                check(
                    WhiskerElementRegistry.bind(
                        elements,
                        listOf(
                            WhiskerElementRegistration(
                                1,
                                WhiskerBuiltInElements.VIEW,
                                WhiskerChildPolicy.Elements,
                                WhiskerMeasurement.None,
                            ),
                        ),
                    ),
                )
                val root = object : WhiskerContainerView(context) {
                    var interceptionDisallowed = false

                    override fun requestDisallowInterceptTouchEvent(disallowIntercept: Boolean) {
                        interceptionDisallowed = disallowIntercept
                        super.requestDisallowInterceptTouchEvent(disallowIntercept)
                    }
                }
                val scene = HostScene(
                    root,
                    context,
                    { _, _, _ -> },
                    { _, _, _ -> },
                    { _ -> },
                    HostRasterResourceStore(),
                    elements,
                )
                fun operation(tag: Int, wide: Long = 0L) = HostSceneOperation(
                    tag = tag,
                    flags = 0,
                    node = 1L,
                    parent = 0L,
                    child = 0L,
                    index = 0,
                    member = if (tag == MobileAbi.OP_CREATE) 1 else 0,
                    integer = 0,
                    scalar = 0f,
                    wide = wide,
                    numbers = null,
                    text = null,
                    names = null,
                    value = null,
                )

                check(scene.beginFrame(0, 1, 0, 1) == 0)
                scene.stage(operation(MobileAbi.OP_CREATE))
                scene.stage(operation(MobileAbi.OP_CAPTURE, 7L))
                assertTrue(scene.commit())
                assertTrue(root.interceptionDisallowed)

                check(scene.beginFrame(1, 1, 1, 2) == 0)
                scene.stage(operation(MobileAbi.OP_RELEASE_CAPTURE, 7L))
                assertTrue(scene.commit())
                assertEquals(false, root.interceptionDisallowed)
            }
    }

    @Test
    fun hiddenContainerDoesNotDispatchToNativeContent() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val node = HostNode(context, WhiskerBuiltInElements.VIEW, null)
                node.addView(
                    View(context).apply { isClickable = true },
                    ViewGroup.LayoutParams(100, 100),
                )
                node.measure(
                    View.MeasureSpec.makeMeasureSpec(100, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(100, View.MeasureSpec.EXACTLY),
                )
                node.layout(0, 0, 100, 100)
                node.setWhiskerVisibility(false)

                val down = MotionEvent.obtain(0L, 0L, MotionEvent.ACTION_DOWN, 50f, 50f, 0)
                try {
                    assertEquals(false, node.dispatchTouchEvent(down))
                } finally {
                    down.recycle()
                }
            }
    }

    @Test
    fun rejectsUnknownElementPropertyAndCommandIds() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                assertTrue(Driver(context, "unknown-members").rejectsUnknownMembers())
            }
    }

    @Test
    fun outerBoxShadowDoesNotPaintInsideTransparentBorderBox() {
        val bitmap = Bitmap.createBitmap(64, 64, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.translate(20f, 20f)
        drawOuterBoxShadows(
            canvas,
            ResolvedBoxGeometry(
                width = 20f,
                height = 20f,
                borderWidths = FloatArray(4),
                cornerRadii = FloatArray(8),
            ),
            listOf(
                HostBoxShadow(
                    offsetX = 0f,
                    offsetY = 0f,
                    blurRadius = 0f,
                    spreadRadius = 5f,
                    color = android.graphics.Color.RED,
                    inset = false,
                ),
            ),
        )

        assertEquals(0, android.graphics.Color.alpha(bitmap.getPixel(30, 30)))
        assertEquals(android.graphics.Color.RED, bitmap.getPixel(17, 30))
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
                    if (id == "core.backdrop-filter-blur" &&
                        Build.VERSION.SDK_INT < Build.VERSION_CODES.S
                    ) return@forEach
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
    fun everySharedMeasurementScenarioUsesTheProductionAndroidProvider() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val manifest = JSONObject(asset("manifest.json"))
                var count = 0
                manifest.getJSONArray("cases").objects().forEach { entry ->
                    val scenario = JSONObject(asset(entry.getString("fixture")))
                    val testSide = scenario.getJSONObject("test")
                    if (testSide.getJSONArray("commands").objects().none {
                            it.getString("type") == "measure_text"
                        }) return@forEach
                    Driver(context, scenario.getString("id")).executeMeasurements(testSide)
                    count += 1
                }
                assertTrue("at least one shared measurement scenario", count > 0)
            }
    }

    @Test
    fun everySharedPointerScenarioUsesTheProductionAndroidNormalizer() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val manifest = JSONObject(asset("manifest.json"))
                var count = 0
                manifest.getJSONArray("cases").objects().forEach { entry ->
                    val scenario = JSONObject(asset(entry.getString("fixture")))
                    val testSide = scenario.getJSONObject("test")
                    if (testSide.getJSONArray("commands").objects().none {
                            it.getString("type") == "emit_pointer"
                        }) return@forEach
                    Driver(context, scenario.getString("id")).executeInput(testSide)
                    count += 1
                }
                assertTrue("at least one shared pointer scenario", count > 0)
            }
    }

    @Test
    fun genericMouseActionsKeepRustHoverStateSynchronized() {
        listOf(
            MotionEvent.ACTION_HOVER_ENTER,
            MotionEvent.ACTION_HOVER_EXIT,
            MotionEvent.ACTION_SCROLL,
        ).forEach { action ->
            val event = MotionEvent.obtain(0L, 0L, action, 10f, 20f, 0)
            try {
                val normalized = normalizePointerInput(event, density = 2f).single()
                assertEquals(1, normalized.event)
                assertEquals(5f, normalized.x, 0.001f)
                assertEquals(10f, normalized.y, 0.001f)
            } finally {
                event.recycle()
            }
        }
    }

    @Test
    fun projectsAThreeDimensionalTransformOntoTheNodePlane() {
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
                assertTrue(Driver(context, "android.transform-3d-projection").commitTransform(matrix))
            }
    }

    @Test
    fun transformedNativeChildReceivesInputAtItsPresentedCoordinates() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val parent = WhiskerContainerView(context)
                val node = HostNode(context, WhiskerBuiltInElements.VIEW, null)
                val content = View(context).apply { isClickable = true }
                node.addView(content, ViewGroup.LayoutParams(100, 100))
                parent.addView(node, ViewGroup.LayoutParams(100, 100))
                parent.measure(
                    View.MeasureSpec.makeMeasureSpec(400, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(200, View.MeasureSpec.EXACTLY),
                )
                parent.layout(0, 0, 400, 200)
                node.setLocalTransform(
                    floatArrayOf(
                        1f, 0f, 0f, 0f,
                        0f, 1f, 0f, 0f,
                        0f, 0f, 1f, 0f,
                        200f, 0f, 0f, 1f,
                    ),
                    density = 1f,
                )

                val down = MotionEvent.obtain(0L, 0L, MotionEvent.ACTION_DOWN, 250f, 50f, 0)
                try {
                    assertTrue(parent.dispatchTouchEvent(down))
                } finally {
                    down.recycle()
                }
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
    fun acceptsHeterogeneousOperationsAsOneTransactionalFrameBatch() {
        androidx.test.platform.app.InstrumentationRegistry
            .getInstrumentation()
            .runOnMainSync {
                val context = ApplicationProvider.getApplicationContext<android.content.Context>()
                val view = WhiskerView(context)
                val metadata = longArrayOf(
                    // create node 1 as the built-in View element
                    1, 0, 1, 0, 0, 0, 1, 0, 0, 0,
                    // assign both border and content geometry
                    6, 0, 1, 0, 0, 0, 0, 0, 0, 0,
                    // apply opacity after layout
                    10, 0, 1, 0, 0, 0, 0, 0, 0.4f.toRawBits().toLong(), 0,
                )
                val response = LongArray(2)

                assertTrue(
                    view.presentFrameFromNative(
                        mode = 0,
                        epoch = 3,
                        baseRevision = 0,
                        targetRevision = 9,
                        metadata = metadata,
                        numbers = arrayOf(
                            null,
                            floatArrayOf(10f, 20f, 30f, 40f, 0f, 0f, 30f, 40f),
                            null,
                        ),
                        texts = arrayOfNulls(3),
                        names = arrayOfNulls(3),
                        values = arrayOfNulls(3),
                        response = response,
                    ),
                )
                assertEquals(0L, response[0])
                assertEquals(9L, response[1])
                assertEquals(9L, view.currentRevisionFromNative())
                assertEquals(0.4f, view.getChildAt(0).alpha, 0.001f)

                val snapshotResponse = LongArray(2)
                assertTrue(
                    view.presentFrameFromNative(
                        mode = 1,
                        epoch = 3,
                        baseRevision = 8,
                        targetRevision = 10,
                        metadata = LongArray(0),
                        numbers = emptyArray(),
                        texts = emptyArray(),
                        names = emptyArray(),
                        values = emptyArray(),
                        response = snapshotResponse,
                    ),
                )
                assertEquals(1L, snapshotResponse[0])
                assertEquals(9L, snapshotResponse[1])
                assertEquals(0.4f, view.getChildAt(0).alpha, 0.001f)

                val rejectedResponse = LongArray(2)
                assertTrue(
                    view.presentFrameFromNative(
                        mode = 1,
                        epoch = 3,
                        baseRevision = 9,
                        targetRevision = 10,
                        metadata = longArrayOf(
                            10, 0, 1, 0, 0, 0, 0, 0,
                            Float.NaN.toRawBits().toLong(), 0,
                        ),
                        numbers = arrayOf(null),
                        texts = arrayOfNulls(1),
                        names = arrayOfNulls(1),
                        values = arrayOfNulls(1),
                        response = rejectedResponse,
                    ),
                )
                assertEquals(2L, rejectedResponse[0])
                assertEquals(9L, rejectedResponse[1])
                assertEquals(0.4f, view.getChildAt(0).alpha, 0.001f)
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
            view.observeRasterResourceEventsForTesting { event ->
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
            view.observeRasterResourceEventsForTesting { unsupported ->
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
    private val measurements = HashMap<Long, FloatArray>()
    private var pointerInput: CapturedPointerInput? = null

    init {
        view.beginBootstrapFromNative()
        view.registerElementFromNative(
            1,
            WhiskerBuiltInElements.VIEW,
            WhiskerChildPolicy.Elements.ordinal,
            WhiskerMeasurement.None.ordinal,
            0,
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
        view.registerElementFromNative(
            2,
            WhiskerBuiltInElements.TEXT,
            WhiskerChildPolicy.PlainText.ordinal,
            WhiskerMeasurement.Text.ordinal,
            0,
            intArrayOf(), intArrayOf(), emptyArray(),
            intArrayOf(), intArrayOf(), emptyArray(),
            intArrayOf(), intArrayOf(), emptyArray(),
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
                    if (command.getString("name") == "paint.text.shadow-single") {
                        val text = checkNotNull(findTextView(view))
                        check(text.text.toString() == "Whisker")
                        check(text.shadowDx == 3f * context.resources.displayMetrics.density)
                        check(text.shadowDy == 4f * context.resources.displayMetrics.density)
                        check(text.shadowRadius == 2f * context.resources.displayMetrics.density)
                    }
                    if (command.getString("name") == "paint.text.decoration-lynx") {
                        val texts = findTextViews(view)
                        check(texts.size == 5)
                        check(texts[0].whiskerDecoration?.style == WhiskerTextDecorationStyle.SOLID)
                        check(texts[1].whiskerDecoration?.style == WhiskerTextDecorationStyle.DOUBLE)
                        check(texts[2].whiskerDecoration?.style == WhiskerTextDecorationStyle.DOTTED)
                        check(texts[3].whiskerDecoration?.style == WhiskerTextDecorationStyle.DASHED)
                        check(texts[4].whiskerDecoration?.style == WhiskerTextDecorationStyle.WAVY)
                        check(texts[0].whiskerDecoration?.line == WhiskerTextDecorationLine.UNDERLINE)
                        check(texts[4].whiskerDecoration?.line == WhiskerTextDecorationLine.LINE_THROUGH)
                    }
                    if (command.getString("name") == "paint.text.align-lynx") {
                        val texts = findTextViews(view)
                        check(texts.size == 5)
                        val horizontal = texts.map { it.gravity and Gravity.RELATIVE_HORIZONTAL_GRAVITY_MASK }
                        check(horizontal == listOf(
                            Gravity.LEFT,
                            Gravity.RIGHT,
                            Gravity.CENTER_HORIZONTAL,
                            Gravity.START,
                            Gravity.END,
                        ))
                        check(texts.map { it.whiskerDirection } == listOf(
                            WhiskerTextDirection.AUTO,
                            WhiskerTextDirection.AUTO,
                            WhiskerTextDirection.AUTO,
                            WhiskerTextDirection.RIGHT_TO_LEFT,
                            WhiskerTextDirection.RIGHT_TO_LEFT,
                        ))
                    }
                    if (command.getString("name") == "paint.text.indent-lynx") {
                        val texts = findTextViews(view)
                        check(texts.size == 2)
                        val density = context.resources.displayMetrics.density
                        val margins = texts.map { text ->
                            val spanned = text.text as Spanned
                            spanned.getSpans(0, spanned.length, LeadingMarginSpan::class.java)
                                .single().getLeadingMargin(true)
                        }
                        check(margins == listOf((24f * density).toInt(), (30f * density).toInt()))
                    }
                    if (command.getString("name") == "paint.text.wrap-overflow-lynx") {
                        val texts = findTextViews(view)
                        check(texts.size == 5)
                        check(texts[1].maxLines == 1)
                        check(texts[2].breakStrategy == android.text.Layout.BREAK_STRATEGY_SIMPLE)
                        check(texts[3].text.contains('\u2060'))
                        check(texts[4].maxLines == 1)
                        check(texts[4].ellipsize == android.text.TextUtils.TruncateAt.END)
                    }
                    if (command.getString("name") == "paint.text.font-features-lynx") {
                        val texts = findTextViews(view)
                        check(texts.size == 3)
                        check(texts[0].whiskerFontFeatures.map { it.tag to it.value } ==
                            listOf("kern" to 0L, "liga" to 1L))
                        check(texts[1].whiskerFontVariations.map { it.tag to it.value } ==
                            listOf("wdth" to 90f, "wght" to 650f))
                        check(texts[2].whiskerFontOpticalSizing == WhiskerFontOpticalSizing.AUTO)
                    }
                    if (command.getString("name") == "paint.text.basic-style-lynx") {
                        val text = findTextViews(view).single()
                        val density = context.resources.displayMetrics.density
                        check(text.whiskerFontFamilies ==
                            listOf("Whisker Fixture Sans", "system"))
                        check(text.whiskerFontStyle == WhiskerFontStyle.ITALIC)
                        check(text.whiskerLineHeight == 28f)
                        check(text.whiskerLetterSpacing == 1.5f)
                        check(abs(text.textSize / density - 20f) < 0.01f)
                        check(text.typeface.isItalic)
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                            check(text.typeface.weight == 650)
                        } else {
                            check(text.typeface.isBold)
                        }
                        check(abs(text.letterSpacing - 0.075f) < 0.0001f)
                        val actualLineHeight =
                            text.paint.fontMetrics.run { descent - ascent } + text.lineSpacingExtra
                        check(abs(actualLineHeight / density - 28f) < 0.01f)
                        check(text.lineSpacingMultiplier == 1f)
                    }
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
                            "paint.visual-effects.box-shadow-blur" ||
                            command.getString("name") ==
                            "paint.visual-effects.box-shadow-inset" ||
                            command.getString("name") ==
                            "paint.visual-effects.box-shadow-multiple" ||
                            command.getString("name") ==
                            "paint.visual-effects.clip-path-inset" ||
                            command.getString("name") ==
                            "paint.visual-effects.clip-path-circle" ||
                            command.getString("name") ==
                            "paint.visual-effects.clip-path-ellipse" ||
                            command.getString("name") ==
                            "paint.visual-effects.clip-path-path-nonzero" ||
                            command.getString("name") ==
                            "paint.visual-effects.clip-path-path-evenodd" ||
                            command.getString("name") ==
                            "paint.visual-effects.backdrop-blur" ||
                            command.getString("name") ==
                            "paint.visual-effects.image-rendering-pixelated" ||
                            command.getString("name") ==
                            "paint.visual-effects.image-rendering-crisp-edges" ||
                            command.getString("name") ==
                            "paint.transform.projective-plane" ||
                            command.getString("name") ==
                            "paint.transform.motion-path-line" ||
                            command.getString("name") ==
                            "paint.transform.motion-path-curves" ||
                            command.getString("name") ==
                            "paint.transform.motion-path-ellipses" ||
                            command.getString("name") ==
                            "paint.transform.motion-path-inset" ||
                            command.getString("name") ==
                            "paint.transform.motion-path-arcs" ||
                            command.getString("name") == "paint.text.shadow-single" ||
                            command.getString("name") == "paint.text.decoration-lynx" ||
                            command.getString("name") == "paint.text.align-lynx" ||
                            command.getString("name") == "paint.text.indent-lynx" ||
                            command.getString("name") == "paint.text.wrap-overflow-lynx" ||
                            command.getString("name") == "paint.text.font-features-lynx" ||
                            command.getString("name") == "paint.text.basic-style-lynx" ||
                            command.getString("name") == "interaction.pointer.lynx",
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

    fun executeMeasurements(side: JSONObject) {
        side.getJSONArray("commands").objects().forEach { command ->
            when (command.getString("type")) {
                "attach_surface" -> {
                    logicalWidth = command.getDouble("width").toFloat()
                    logicalHeight = command.getDouble("height").toFloat()
                }
                "measure_text" -> measureText(command)
                "checkpoint_measurement" -> checkpointMeasurement(command)
                else -> error("unsupported Android measurement command: ${command.getString("type")}")
            }
        }
    }

    fun executeInput(side: JSONObject) {
        side.getJSONArray("commands").objects().forEach { command ->
            when (command.getString("type")) {
                "attach_surface" -> {
                    logicalWidth = command.getDouble("width").toFloat()
                    logicalHeight = command.getDouble("height").toFloat()
                }
                "emit_pointer" -> emitPointer(command)
                "checkpoint_input" -> checkpointInput(command)
                else -> error("unsupported Android pointer command: ${command.getString("type")}")
            }
        }
    }

    private fun emitPointer(command: JSONObject) {
        val eventKind = when (command.getString("event")) {
            "down" -> MotionEvent.ACTION_DOWN
            "move" -> MotionEvent.ACTION_MOVE
            "up" -> MotionEvent.ACTION_UP
            "cancel" -> MotionEvent.ACTION_CANCEL
            else -> error("unsupported pointer event: $command")
        }
        val pointerId = command.getLong("pointer_id")
        check(pointerId in 1..Int.MAX_VALUE.toLong())
        val density = context.resources.displayMetrics.density
        val (toolType, source) = when (command.getString("pointer_kind")) {
            "mouse" -> MotionEvent.TOOL_TYPE_MOUSE to InputDevice.SOURCE_MOUSE
            "touch" -> MotionEvent.TOOL_TYPE_FINGER to InputDevice.SOURCE_TOUCHSCREEN
            "pen" -> MotionEvent.TOOL_TYPE_STYLUS to InputDevice.SOURCE_STYLUS
            "unknown" -> MotionEvent.TOOL_TYPE_UNKNOWN to InputDevice.SOURCE_UNKNOWN
            else -> error("unsupported pointer kind: $command")
        }
        val properties = arrayOf(MotionEvent.PointerProperties().apply {
            id = (pointerId - 1).toInt()
            this.toolType = toolType
        })
        val coordinates = arrayOf(MotionEvent.PointerCoords().apply {
            x = command.getDouble("x").toFloat() * density
            y = command.getDouble("y").toFloat() * density
            pressure = 1f
            size = 1f
        })
        val timestampMs = command.getDouble("timestamp_ms").toLong()
        val event = MotionEvent.obtain(
            timestampMs,
            timestampMs,
            eventKind,
            1,
            properties,
            coordinates,
            0,
            command.getInt("buttons"),
            1f,
            1f,
            0,
            0,
            source,
            0,
        )
        try {
            val normalized = normalizePointerInput(event, density).single()
            pointerInput = CapturedPointerInput(
                event = normalized.event,
                pointerId = normalized.pointerId,
                kind = normalized.kind,
                buttons = normalized.buttons,
                changedButton = normalized.changedButton,
                timestampMs = normalized.timestampMs,
                x = normalized.x,
                y = normalized.y,
            )
            view.dispatchTouchEvent(event)
        } finally {
            event.recycle()
        }
        val normalized = checkNotNull(pointerInput)
        check(normalized.buttons == command.getInt("buttons"))
        check(normalized.changedButton == command.getInt("changed_button"))
    }

    private fun checkpointInput(command: JSONObject) {
        val normalized = checkNotNull(pointerInput)
        val expectedEvent = when (command.getString("event")) {
            "down" -> 0
            "move" -> 1
            "up" -> 2
            "cancel" -> 3
            else -> error("unsupported pointer checkpoint: $command")
        }
        check(normalized.event == expectedEvent)
        check(normalized.pointerId == command.getLong("pointer_id"))
        val expectedKind = when (command.getString("pointer_kind")) {
            "mouse" -> 0
            "touch" -> 1
            "pen" -> 2
            "unknown" -> 3
            else -> error("unsupported pointer kind: $command")
        }
        check(normalized.kind == expectedKind)
        check(abs(normalized.x - command.getDouble("x").toFloat()) < 0.001f)
        check(abs(normalized.y - command.getDouble("y").toFloat()) < 0.001f)
    }

    private fun measureText(command: JSONObject) {
        val families = command.optJSONArray("font_families")?.strings() ?: arrayOf("system")
        check(families.isNotEmpty())
        val featureSettings = command.optJSONArray("font_features")?.objects()?.map { setting ->
            "${setting.getString("tag")}=${setting.getLong("value")}"
        }?.toList().orEmpty()
        val variationSettings = command.optJSONArray("font_variations")?.objects()?.map { setting ->
            "${setting.getString("tag")}=${setting.getDouble("value")}"
        }?.toList().orEmpty()
        val opticalSizing = when (command.optString("font_optical_sizing", "none")) {
            "auto" -> 0
            "none" -> 1
            else -> error("unsupported font_optical_sizing: $command")
        }
        val wrap = when (command.optString("white_space", "normal")) {
            "normal" -> 1
            "no_wrap" -> 0
            else -> error("unsupported white_space: $command")
        }
        val wordBreak = when (command.optString("word_break", "normal")) {
            "normal" -> 0
            "break_all" -> 1
            "keep_all" -> 2
            else -> error("unsupported word_break: $command")
        }
        val overflow = when (command.optString("overflow", "clip")) {
            "clip" -> 0
            "ellipsis" -> 1
            else -> error("unsupported overflow: $command")
        }
        val maxLines = command.optInt("max_lines", 0)
        check(maxLines >= 0)
        val direction = when (command.optString("direction", "auto")) {
            "auto" -> 0
            "left_to_right" -> 1
            "right_to_left" -> 2
            else -> error("unsupported direction: $command")
        }
        val alignment = when (command.optString("alignment", "start")) {
            "start" -> 0
            "end" -> 1
            "left" -> 2
            "right" -> 3
            "center" -> 4
            else -> error("unsupported alignment: $command")
        }
        val indent = command.optJSONObject("indent")
        val indentLogicalPixels = indent?.optDouble("logical_pixels", 0.0)?.toFloat() ?: 0f
        val indentPercentage = indent?.optDouble("percentage", 0.0)?.toFloat() ?: 0f
        val requestInts = IntArray(HostMeasureBatchAbi.REQUEST_INT_STRIDE).apply {
            this[HostMeasureBatchAbi.ELEMENT_TYPE] = 2
            this[HostMeasureBatchAbi.KIND] = 1
            this[HostMeasureBatchAbi.AVAILABLE_WIDTH_KIND] = when (
                command.optString("available_width_kind", "definite")
            ) {
                "definite" -> 0
                "min_content" -> 1
                "max_content" -> 2
                else -> error("unsupported available_width_kind: $command")
            }
            this[HostMeasureBatchAbi.AVAILABLE_HEIGHT_KIND] = 2
            this[HostMeasureBatchAbi.FONT_WEIGHT] = command.optInt("font_weight", 400)
            this[HostMeasureBatchAbi.FONT_STYLE] = when (command.optString("font_style", "normal")) {
                "italic" -> 1
                "oblique" -> 2
                "normal" -> 0
                else -> error("unsupported font_style: $command")
            }
            this[HostMeasureBatchAbi.WRAP] = wrap
            this[HostMeasureBatchAbi.WORD_BREAK] = wordBreak
            this[HostMeasureBatchAbi.OVERFLOW] = overflow
            this[HostMeasureBatchAbi.MAX_LINES] = maxLines
            this[HostMeasureBatchAbi.FONT_FEATURE_COUNT] = featureSettings.size
            this[HostMeasureBatchAbi.FONT_OPTICAL_SIZING] = opticalSizing
            this[HostMeasureBatchAbi.DIRECTION] = direction
            this[HostMeasureBatchAbi.ALIGNMENT] = alignment
        }
        val requestFloats = FloatArray(HostMeasureBatchAbi.REQUEST_FLOAT_STRIDE).apply {
            this[HostMeasureBatchAbi.AVAILABLE_WIDTH] =
                command.getDouble("available_width").toFloat()
            this[HostMeasureBatchAbi.AVAILABLE_HEIGHT] = Float.POSITIVE_INFINITY
            this[HostMeasureBatchAbi.FONT_SIZE] = command.getDouble("font_size").toFloat()
            this[HostMeasureBatchAbi.LINE_HEIGHT] = command.getDouble("line_height").toFloat()
            this[HostMeasureBatchAbi.LETTER_SPACING] =
                command.optDouble("letter_spacing", 0.0).toFloat()
            this[HostMeasureBatchAbi.INDENT_LOGICAL_PIXELS] = indentLogicalPixels
            this[HostMeasureBatchAbi.INDENT_PERCENTAGE] = indentPercentage
        }
        val batch = view.measureBatchFromNative(
            longArrayOf(command.getLong("key"), 1L, 1L),
            requestInts,
            requestFloats,
            arrayOf(command.getString("text"), command.optString("locale", "")),
            arrayOf(families),
            arrayOf((featureSettings + variationSettings).toTypedArray()),
            arrayOf(byteArrayOf()),
        )
        val result = floatArrayOf(
            batch.ints[0].toFloat(),
            batch.ints[1].toFloat(),
            batch.floats[0],
            batch.floats[1],
            batch.floats[2],
            batch.floats[3],
            batch.ints[2].toFloat(),
        )
        check(result.size >= 7 && result[0] == 1f) { "$id text measurement was not ready" }
        if (id == "host.measure.text.font-features") {
            check(featureSettings == listOf("kern=0", "liga=0"))
            check(variationSettings == listOf("wght=720.0"))
            check(opticalSizing == 1)
        }
        if (id == "host.measure.text.direction") {
            val density = context.resources.displayMetrics.density
            val semantics = resolveTextLayoutSemantics(
                text = command.getString("text"),
                direction = direction,
                alignment = alignment,
                localeRtl = context.resources.configuration.layoutDirection ==
                    View.LAYOUT_DIRECTION_RTL,
                widthBasis = command.getDouble("available_width").toFloat(),
                density = density,
                indentLogicalPixels = indentLogicalPixels,
                indentPercentage = indentPercentage,
            )
            check(abs(semantics.indentPixels / density - (indentLogicalPixels +
                command.getDouble("available_width").toFloat() * indentPercentage / 100f)) < 0.01f)
            when (command.getLong("key")) {
                20L -> {
                    check(semantics.directionHeuristic === TextDirectionHeuristics.RTL)
                    check(semantics.alignment == android.text.Layout.Alignment.ALIGN_OPPOSITE)
                }
                21L -> {
                    check(semantics.directionHeuristic === TextDirectionHeuristics.LTR)
                    check(semantics.alignment == android.text.Layout.Alignment.ALIGN_CENTER)
                }
            }
        }
        measurements[command.getLong("key")] = result
    }

    private fun checkpointMeasurement(command: JSONObject) {
        val result = checkNotNull(measurements[command.getLong("key")])
        val width = result[2]
        val height = result[3]
        val minWidth = command.getDouble("min_width").toFloat()
        val maxWidth = command.getDouble("max_width").toFloat()
        val minHeight = command.getDouble("min_height").toFloat()
        val maxHeight = command.getDouble("max_height").toFloat()
        check(width.isFinite() && width >= minWidth) {
            "$id measured width $width is below $minWidth"
        }
        check(width <= maxWidth) { "$id measured width $width exceeds $maxWidth" }
        check(height.isFinite() && height >= minHeight) {
            "$id measured height $height is below $minHeight"
        }
        check(height <= maxHeight) { "$id measured height $height exceeds $maxHeight" }
        // MobileMeasureResponse has a prepared-content ID, but the current Android
        // TextView measurer does not create reusable prepared content. The shared
        // fixture therefore leaves that optional optimization unconstrained.
    }

    fun commitTransform(transform: FloatArray): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        check(stage(tag = 9, numbers = transform))
        return view.commitFrameForTesting()
    }

    fun acceptsBackdropBlur(radius: Float): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        val accepted = stage(tag = 24, scalar = radius)
        val committed = view.commitFrameForTesting()
        return accepted && committed
    }

    fun acceptsPointerCapture(): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        check(stage(tag = MobileAbi.OP_CAPTURE, wide = 7))
        check(stage(tag = MobileAbi.OP_RELEASE_CAPTURE, wide = 7))
        return view.commitFrameForTesting()
    }

    fun verifyPhysicalZOrder(): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        repeat(3) { index ->
            val node = (index + 1).toLong()
            check(stage(tag = MobileAbi.OP_CREATE, node = node, member = 1))
            check(
                stage(
                    tag = MobileAbi.OP_LAYOUT,
                    node = node,
                    numbers = floatArrayOf(node.toFloat(), 0f, 10f, 10f, 0f, 0f, 10f, 10f),
                ),
            )
        }
        check(stage(tag = MobileAbi.OP_Z_ORDER, node = 1, integer = 10))
        check(stage(tag = MobileAbi.OP_Z_ORDER, node = 2, integer = -5))
        check(stage(tag = MobileAbi.OP_Z_ORDER, node = 3, integer = 10))
        check(view.commitFrameForTesting())

        val orderedNodes = (0 until view.childCount).map { view.getChildAt(it) as HostNode }
        return orderedNodes.map { it.geometry.x } == listOf(2f, 1f, 3f) &&
            orderedNodes.all { it.translationZ == 0f }
    }

    fun rejectsUnknownMembers(): Boolean {
        val cases = listOf(
            Triple(MobileAbi.OP_PROPERTY, 999, WhiskerValue.Int(1)),
            Triple(MobileAbi.OP_CLEAR_PROPERTY, 999, null),
            Triple(MobileAbi.OP_COMMAND, 999, WhiskerValue.Null),
        )
        return cases.all { (tag, member, value) ->
            check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
            check(stage(tag = MobileAbi.OP_CREATE, member = 1))
            check(stage(tag = tag, member = member, value = value))
            !view.commitFrameForTesting()
        }
    }

    fun verifyLayoutRounding(): Boolean {
        val density = context.resources.displayMetrics.density
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        check(stage(tag = MobileAbi.OP_CREATE, member = 1))
        check(
            stage(
                tag = MobileAbi.OP_LAYOUT,
                numbers = floatArrayOf(
                    0f,
                    0f,
                    10.75f / density,
                    12.75f / density,
                    0f,
                    0f,
                    6.75f / density,
                    8.75f / density,
                ),
            ),
        )
        check(view.commitFrameForTesting())
        val node = view.getChildAt(0) as HostNode
        val content = checkNotNull(node.mountedElement).view
        return node.layoutParams.width == 11 && node.layoutParams.height == 13 &&
            content.layoutParams.width == 7 && content.layoutParams.height == 9
    }

    fun rejectOpacity(opacity: Float): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
        check(stage(tag = 1, member = 1))
        check(stage(tag = 10, scalar = opacity))
        return !view.commitFrameForTesting()
    }

    fun rejectUnregisteredRasterResource(resourceId: Long): Boolean {
        return !commitRasterResource(resourceId)
    }

    fun acceptRasterResource(resourceId: Long): Boolean {
        val bitmap = Bitmap.createBitmap(1, 1, Bitmap.Config.ARGB_8888)
        check(view.registerRasterResourceForTesting(resourceId, bitmap))
        return commitRasterResource(resourceId)
    }

    fun reportsRasterDecodeFailure(): Boolean {
        check(
            view.loadRasterResourceBytesForTesting(
                7L,
                1L,
                "image/png",
                byteArrayOf(0, 1, 2, 3),
            ),
        )
        return view.awaitRasterResourceForTesting(7L, 1L, 5_000)?.state ==
            HostResourceState.Failed
    }

    private fun commitRasterResource(resourceId: Long): Boolean {
        check(view.beginFrameForTesting(0, 1, 0, 1) == 0)
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
        return view.commitFrameForTesting()
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
        check(view.registerRasterResourceForTesting(command.getLong("id"), bitmap))
    }

    private fun loadRasterResource(command: JSONObject) {
        val resourceId = command.getLong("id")
        val generation = command.getLong("generation")
        val source = command.getJSONObject("source")
        val accepted = when (source.getString("kind")) {
            "bytes" -> view.loadRasterResourceBytesForTesting(
                resourceId,
                generation,
                source.getString("media_type"),
                Base64.decode(source.getString("base64"), Base64.DEFAULT),
            )
            "url" -> view.loadRasterResourceUrlForTesting(
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
            view.releaseRasterResourceForTesting(
                command.getLong("id"),
                command.getLong("generation"),
            ),
        )
    }

    private fun checkpointRasterResource(command: JSONObject) {
        val snapshot = checkNotNull(
            view.awaitRasterResourceForTesting(
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
        check(view.beginFrameForTesting(0, 1, 0, revision) == 0)
        check(stage(tag = 1, member = 1))
        val rect = command.getJSONArray("rect").floats()
        check(stage(tag = 6, numbers = rect + floatArrayOf(0f, 0f, 0f, 0f)))
        val (numbers, names) = paint(command)
        check(stage(tag = 7, numbers = numbers, names = names))
        check(view.commitFrameForTesting()) { "$id rejected present_box" }
    }

    private fun presentScene(command: JSONObject) {
        val revision = command.getLong("revision")
        val nodes = command.getJSONArray("nodes")
        check(view.beginFrameForTesting(0, 1, 0, revision) == 0)
        nodes.objects().forEach { node ->
            check(stage(tag = 1, node = node.getLong("id"), member = if (node.has("text")) 2 else 1))
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
            node.optJSONObject("text")?.let { text ->
                val textNumbers = ArrayList<Float>(37)
                val textNames = ArrayList<String>(3)
                textNumbers += text.getDouble("font_size").toFloat()
                textNumbers += text.optInt("font_weight", 400).toFloat()
                textNumbers += when (text.optString("font_style", "normal")) {
                    "italic" -> 1f
                    "oblique" -> 2f
                    else -> 0f
                }
                appendColor(text.getJSONObject("color"), textNumbers, textNames)
                val shadow = text.optJSONObject("shadow")
                textNumbers += if (shadow == null) 0f else 1f
                val offset = shadow?.getJSONArray("offset")
                textNumbers += offset?.getDouble(0)?.toFloat() ?: 0f
                textNumbers += offset?.getDouble(1)?.toFloat() ?: 0f
                textNumbers += shadow?.getDouble("blur_radius")?.toFloat() ?: 0f
                appendColor(
                    shadow?.getJSONObject("color")
                        ?: JSONObject("{\"kind\":\"srgba\",\"red\":0,\"green\":0,\"blue\":0,\"alpha\":0}"),
                    textNumbers,
                    textNames,
                )
                val decoration = text.optJSONObject("decoration")
                textNumbers += when (decoration?.getString("line")) {
                    "underline" -> 1f
                    "line_through" -> 2f
                    else -> 0f
                }
                textNumbers += when (decoration?.getString("style")) {
                    "double" -> 1f
                    "dotted" -> 2f
                    "dashed" -> 3f
                    "wavy" -> 4f
                    else -> 0f
                }
                appendColor(
                    decoration?.getJSONObject("color")
                        ?: JSONObject("{\"kind\":\"srgba\",\"red\":0,\"green\":0,\"blue\":0,\"alpha\":0}"),
                    textNumbers,
                    textNames,
                )
                textNumbers += when (text.optString("alignment", "start")) {
                    "end" -> 1f
                    "left" -> 2f
                    "right" -> 3f
                    "center" -> 4f
                    else -> 0f
                }
                val indent = text.optJSONObject("indent")
                textNumbers += indent?.optDouble("logical_pixels", 0.0)?.toFloat() ?: 0f
                textNumbers += indent?.optDouble("percentage", 0.0)?.toFloat() ?: 0f
                textNumbers += if (text.optString("white_space", "normal") == "normal") 1f else 0f
                textNumbers += when (text.optString("word_break", "normal")) {
                    "break_all" -> 1f
                    "keep_all" -> 2f
                    else -> 0f
                }
                textNumbers += text.optInt("max_lines", 0).toFloat()
                textNumbers += if (text.optString("overflow", "clip") == "ellipsis") 1f else 0f
                textNumbers += if (text.optString("font_optical_sizing", "none") == "auto") 0f else 1f
                val features = text.optJSONArray("font_features")
                textNumbers += (features?.length() ?: 0).toFloat()
                val families = text.optJSONArray("font_families") ?: JSONArray("[\"system\"]")
                textNumbers += families.length().toFloat()
                textNumbers += text.optDouble("line_height", 0.0).toFloat()
                textNumbers += text.optDouble("letter_spacing", 0.0).toFloat()
                textNumbers += when (text.optString("direction", "auto")) {
                    "left_to_right" -> 1f
                    "right_to_left" -> 2f
                    else -> 0f
                }
                families.strings().forEach(textNames::add)
                features?.objects()?.forEach { setting ->
                    textNames += "${setting.getString("tag")}=${setting.getLong("value")}"
                }
                text.optJSONArray("font_variations")?.objects()?.forEach { setting ->
                    textNames += "${setting.getString("tag")}=${setting.getDouble("value")}"
                }
                check(stage(
                    tag = 13,
                    node = id,
                    text = text.getString("value"),
                    numbers = textNumbers.toFloatArray(),
                    names = textNames.toTypedArray(),
                ))
            }
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
            node.optJSONObject("clip_path")?.let { clip ->
                val shape = clip.getJSONObject("shape")
                val numbers = ArrayList<Float>(26)
                numbers += backgroundBox(clip.optString("reference_box", "border")).toFloat()
                when (shape.getString("kind")) {
                    "circle" -> {
                        numbers += 1f
                        appendLengthPercentage(shape.getJSONObject("radius"), numbers)
                        shape.getJSONArray("center").objects().forEach {
                            appendLengthPercentage(it, numbers)
                        }
                    }
                    "ellipse" -> {
                        numbers += 2f
                        shape.getJSONArray("radii").objects().forEach {
                            appendLengthPercentage(it, numbers)
                        }
                        shape.getJSONArray("center").objects().forEach {
                            appendLengthPercentage(it, numbers)
                        }
                    }
                    "path" -> {
                        numbers += 3f
                        numbers += if (shape.optString("fill_rule", "non_zero") == "even_odd") 1f else 0f
                        val commands = shape.getJSONArray("commands")
                        numbers += commands.length().toFloat()
                        commands.objects().forEach { command ->
                            val pointNames = when (command.getString("command")) {
                                "move_to" -> 0 to listOf("point")
                                "line_to" -> 1 to listOf("point")
                                "quadratic_to" -> 2 to listOf("control", "end")
                                "cubic_to" -> 3 to listOf("control_1", "control_2", "end")
                                "close" -> 4 to emptyList()
                                else -> error("unsupported path command")
                            }
                            numbers += pointNames.first.toFloat()
                            pointNames.second.forEach { name ->
                                command.getJSONArray(name).objects().forEach {
                                    appendLengthPercentage(it, numbers)
                                }
                            }
                            repeat(6 - pointNames.second.size * 2) {
                                appendLengthPercentage(JSONObject().put("length", 0), numbers)
                            }
                        }
                    }
                    else -> {
                        numbers += 0f
                        shape.getJSONArray("edges").objects().forEach { edge ->
                            appendLengthPercentage(edge, numbers)
                        }
                        val radii = shape.getJSONArray("radii")
                        repeat(2) { axis ->
                            repeat(4) { index ->
                                val radius = radii.get(index)
                                numbers += when (radius) {
                                    is Number -> radius.toFloat()
                                    is JSONArray -> radius.getDouble(axis).toFloat()
                                    else -> error("unsupported clip radius: $radius")
                                }
                                numbers += 0f
                            }
                        }
                    }
                }
                check(stage(tag = 23, node = id, numbers = numbers.toFloatArray()))
            }
            if (node.has("backdrop_blur")) {
                check(
                    stage(
                        tag = 24,
                        node = id,
                        scalar = node.getDouble("backdrop_blur").toFloat(),
                    ),
                )
            }
            if (node.has("image_rendering")) {
                val value = when (node.getString("image_rendering")) {
                    "pixelated" -> 1
                    "crisp_edges" -> 2
                    else -> 0
                }
                check(stage(tag = 25, node = id, integer = value))
            }
            val cursor = when (node.optString("cursor", "auto")) {
                "pointer" -> 5
                "text" -> 10
                "grab" -> 17
                "none" -> 2
                else -> 0
            }
            check(stage(tag = 26, node = id, integer = cursor))
            check(
                stage(
                    tag = 17,
                    node = id,
                    integer = if (node.optString("pointer_events", "auto") == "none") 1 else 0,
                ),
            )
        }
        check(view.commitFrameForTesting()) { "$id: Host rejected the staged scene frame" }
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
        wide: Long = 0,
        numbers: FloatArray? = null,
        text: String? = null,
        names: Array<String>? = null,
        value: WhiskerValue? = null,
    ): Boolean = view.stageOperationForTesting(
        tag,
        flags,
        node,
        parent,
        child,
        index,
        member,
        integer,
        scalar,
        wide,
        numbers,
        text,
        names,
        value,
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

private fun findTextView(view: android.view.View): TextView? {
    if (view is TextView) return view
    if (view is ViewGroup) {
        for (index in 0 until view.childCount) {
            findTextView(view.getChildAt(index))?.let { return it }
        }
    }
    return null
}

private fun findTextViews(view: android.view.View): List<WhiskerTextView> {
    val result = ArrayList<WhiskerTextView>()
    fun visit(candidate: android.view.View) {
        if (candidate is WhiskerTextView) result += candidate
        if (candidate is ViewGroup) {
            for (index in 0 until candidate.childCount) visit(candidate.getChildAt(index))
        }
    }
    visit(view)
    return result
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

private fun hostOperation(
    tag: Int,
    node: Long = 0,
    parent: Long = 0,
    child: Long = 0,
    member: Int = 0,
    numbers: FloatArray? = null,
): HostSceneOperation = HostSceneOperation(
    tag = tag,
    flags = 0,
    node = node,
    parent = parent,
    child = child,
    index = 0,
    member = member,
    integer = 0,
    scalar = 0f,
    wide = 0,
    numbers = numbers,
    text = null,
    names = null,
    value = null,
)
