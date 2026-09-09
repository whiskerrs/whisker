package rs.whisker.runtime

import android.graphics.Color
import android.view.View
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class TextSelectionTest {
    @Test
    fun selectionQueriesUseNativeUtf16RangesAndRejectStaleRevision() {
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val registration = WhiskerElementRegistration(
                elementType = 2, name = WhiskerBuiltInElements.TEXT,
                childPolicy = WhiskerChildPolicy.RichText, measurement = WhiskerMeasurement.Text,
                properties = listOf(WhiskerPropertyBinding(1, "selectable", WhiskerValueKind.Bool)),
                events = listOf(WhiskerEventBinding(1, "selectionchange", WhiskerValueKind.Map), WhiskerEventBinding(2, "textqueryresult", WhiskerValueKind.Map), WhiskerEventBinding(3, "textactivate", WhiskerValueKind.Map)),
                commands = listOf(WhiskerCommandBinding(1, "setSelection", WhiskerValueKind.Map), WhiskerCommandBinding(2, "clearSelection", WhiskerValueKind.Map), WhiskerCommandBinding(3, "textQuery", WhiskerValueKind.Map)),
            )
            val elements = WhiskerElementRegistry.newBindings()
            assertTrue(WhiskerElementRegistry.bind(elements, listOf(registration)))
            val replies = mutableListOf<Map<String, WhiskerValue>>()
            val mounted = checkNotNull(elements.mount(2, ApplicationProvider.getApplicationContext()) { event, detail ->
                if (event.name == "textqueryresult") replies += (detail as WhiskerValue.Map).value
            })
            mounted.view.layoutParams = android.view.ViewGroup.LayoutParams(500, 120)
            mounted.setEventMask(3)
            val content = WhiskerTextContent(value = "Hello 🦀 world", preparedContent = 7, fontSize = 20f, fontWeight = 400, color = Color.BLACK)
            assertTrue(mounted.setText(content))
            mounted.setProperty(1, WhiskerValue.Bool(true))
            mounted.view.measure(View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(120, View.MeasureSpec.EXACTLY))
            mounted.view.layout(0, 0, 500, 120)
            mounted.invokeCommand(1, WhiskerValue.Map(mapOf("revision" to WhiskerValue.Int(7), "start" to WhiskerValue.Int(6), "end" to WhiskerValue.Int(8))))
            fun query(revision: Long, kind: String) {
                mounted.invokeCommand(3, WhiskerValue.Map(mapOf(
                    "id" to WhiskerValue.Int(9), "revision" to WhiskerValue.Int(revision), "kind" to WhiskerValue.Str(kind),
                    "start" to WhiskerValue.Int(6), "end" to WhiskerValue.Int(8),
                )))
            }
            query(7, "selectedText")
            assertEquals("🦀", replies.last()["text"]?.asString())
            query(7, "boundingRects")
            assertTrue((replies.last()["rects"] as WhiskerValue.Array).value.isNotEmpty())
            query(8, "selectedText")
            assertEquals("stale-layout", replies.last()["error"]?.asString())
            assertTrue(mounted.setText(content.copy(color = Color.RED)))
            query(7, "selectedText")
            assertEquals("🦀", replies.last()["text"]?.asString())
            assertTrue(mounted.setText(content.copy(value = "Changed", preparedContent = 8)))
            query(8, "selectedText")
            assertEquals("", replies.last()["text"]?.asString())
        }
    }
}
