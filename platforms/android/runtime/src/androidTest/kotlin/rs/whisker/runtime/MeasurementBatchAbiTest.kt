package rs.whisker.runtime

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertArrayEquals
import org.junit.Test
import org.junit.runner.RunWith
import rs.whisker.runtime.measure.HostMeasureBatchAbi

@RunWith(AndroidJUnit4::class)
class MeasurementBatchAbiTest {
    @Test
    fun measurementBatchPreservesRequestOrderAndResponseFields() {
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val view = WhiskerView(
                ApplicationProvider.getApplicationContext<android.content.Context>(),
            )
            val requestLongs = longArrayOf(
                11L, 101L, 201L,
                12L, 102L, 202L,
            )
            val requestInts = IntArray(2 * HostMeasureBatchAbi.REQUEST_INT_STRIDE).apply {
                this[HostMeasureBatchAbi.ELEMENT_TYPE] = 41
                this[HostMeasureBatchAbi.KIND] = 2
                this[HostMeasureBatchAbi.AVAILABLE_WIDTH_KIND] = 2
                this[HostMeasureBatchAbi.AVAILABLE_HEIGHT_KIND] = 1
                this[HostMeasureBatchAbi.FONT_WEIGHT] = 400
                this[HostMeasureBatchAbi.WRAP] = 1
                this[HostMeasureBatchAbi.PAYLOAD_VERSION] = 7
                this[HostMeasureBatchAbi.INTRINSIC_MASK] = 3
                this[HostMeasureBatchAbi.DIRECTION] = 1
                this[HostMeasureBatchAbi.ALIGNMENT] = 2

                val second = HostMeasureBatchAbi.REQUEST_INT_STRIDE
                this[second + HostMeasureBatchAbi.ELEMENT_TYPE] = 42
                this[second + HostMeasureBatchAbi.KIND] = 2
                this[second + HostMeasureBatchAbi.KNOWN_MASK] = 3
                this[second + HostMeasureBatchAbi.AVAILABLE_WIDTH_KIND] = 1
                this[second + HostMeasureBatchAbi.AVAILABLE_HEIGHT_KIND] = 2
                this[second + HostMeasureBatchAbi.FONT_WEIGHT] = 700
                this[second + HostMeasureBatchAbi.FONT_STYLE] = 1
                this[second + HostMeasureBatchAbi.WORD_BREAK] = 2
                this[second + HostMeasureBatchAbi.OVERFLOW] = 1
                this[second + HostMeasureBatchAbi.MAX_LINES] = 3
                this[second + HostMeasureBatchAbi.PAYLOAD_VERSION] = 9
                this[second + HostMeasureBatchAbi.INTRINSIC_MASK] = 3
                this[second + HostMeasureBatchAbi.DIRECTION] = 2
                this[second + HostMeasureBatchAbi.ALIGNMENT] = 4
            }
            val requestFloats = FloatArray(2 * HostMeasureBatchAbi.REQUEST_FLOAT_STRIDE).apply {
                this[HostMeasureBatchAbi.AVAILABLE_WIDTH] = 37f
                this[HostMeasureBatchAbi.AVAILABLE_HEIGHT] = 41f
                this[HostMeasureBatchAbi.FONT_SIZE] = 13f
                this[HostMeasureBatchAbi.LINE_HEIGHT] = 17f
                this[HostMeasureBatchAbi.LETTER_SPACING] = 2f
                this[HostMeasureBatchAbi.INDENT_LOGICAL_PIXELS] = 3f
                this[HostMeasureBatchAbi.INDENT_PERCENTAGE] = 4f
                this[HostMeasureBatchAbi.INTRINSIC_WIDTH] = 43f
                this[HostMeasureBatchAbi.INTRINSIC_HEIGHT] = 47f

                val second = HostMeasureBatchAbi.REQUEST_FLOAT_STRIDE
                this[second + HostMeasureBatchAbi.KNOWN_WIDTH] = 71f
                this[second + HostMeasureBatchAbi.KNOWN_HEIGHT] = 73f
                this[second + HostMeasureBatchAbi.AVAILABLE_WIDTH] = 79f
                this[second + HostMeasureBatchAbi.AVAILABLE_HEIGHT] = 83f
                this[second + HostMeasureBatchAbi.FONT_SIZE] = 19f
                this[second + HostMeasureBatchAbi.LINE_HEIGHT] = 23f
                this[second + HostMeasureBatchAbi.LETTER_SPACING] = 5f
                this[second + HostMeasureBatchAbi.INDENT_LOGICAL_PIXELS] = 6f
                this[second + HostMeasureBatchAbi.INDENT_PERCENTAGE] = 7f
                this[second + HostMeasureBatchAbi.INTRINSIC_WIDTH] = 89f
                this[second + HostMeasureBatchAbi.INTRINSIC_HEIGHT] = 97f
            }

            val response = view.measureBatchFromNative(
                requestLongs,
                requestInts,
                requestFloats,
                arrayOf("first", "ja-JP", "second", "en-US"),
                arrayOf(arrayOf("sans-serif"), arrayOf("serif")),
                arrayOf(arrayOf("kern=1"), arrayOf("wght=700")),
                arrayOf(byteArrayOf(1, 2), byteArrayOf(3, 4, 5)),
            )

            assertArrayEquals(
                longArrayOf(11L, 201L, 0L, 0L, 12L, 202L, 0L, 0L),
                response.longs,
            )
            assertArrayEquals(
                intArrayOf(1, 0, 0, 1, 0, 0),
                response.ints,
            )
            assertArrayEquals(
                floatArrayOf(43f, 47f, 0f, 0f, 71f, 73f, 0f, 0f),
                response.floats,
                0f,
            )
        }
    }
}
