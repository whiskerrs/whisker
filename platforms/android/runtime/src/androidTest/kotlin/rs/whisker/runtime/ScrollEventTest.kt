package rs.whisker.runtime

import android.content.Intent
import android.os.SystemClock
import android.view.MotionEvent
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ScrollEventTest {
    private fun gesture(horizontal: Boolean, cancel: Boolean = false, clickable: Boolean = false, tap: Boolean = false, atEdge: Boolean = false) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val intent = Intent(instrumentation.context, ScrollCancellationActivity::class.java)
            .putExtra("horizontal", horizontal).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ActivityScenario.launch<ScrollCancellationActivity>(intent).use { scenario ->
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                val events = mutableListOf<Map<String, WhiskerValue>>()
                activity.scroll.contentView.getChildAt(0).isClickable = clickable
                activity.scroll.installWhiskerEventSink { name, value ->
                    if (name == "scroll") events.add((value as WhiskerValue.Map).value)
                }
                val now = SystemClock.uptimeMillis()
                fun send(action: Int, time: Long, position: Float) {
                    val coordinate = if (atEdge) 300f - position else position
                    val event = MotionEvent.obtain(now, now + time, action,
                        if (horizontal) coordinate else 100f, if (horizontal) 100f else coordinate, 0)
                    activity.scroll.dispatchTouchEvent(event)
                    event.recycle()
                }
                send(MotionEvent.ACTION_DOWN, 0, 250f)
                if (!tap) {
                    send(MotionEvent.ACTION_MOVE, 20, 200f)
                    send(MotionEvent.ACTION_MOVE, 40, 150f)
                    send(MotionEvent.ACTION_MOVE, 60, 100f)
                }
                val countBeforeEnd = events.size
                send(if (cancel) MotionEvent.ACTION_CANCEL else MotionEvent.ACTION_UP, 70, if (tap) 250f else 75f)
                if (tap) {
                    assertTrue("a tap is not a drag", events.isEmpty())
                } else {
                    assertEquals("release must notify even without offset movement", countBeforeEnd + 1, events.size)
                    assertEquals(WhiskerValue.Bool(true), events.first()["isDragging"])
                    val end = events.last()
                    assertEquals(WhiskerValue.Bool(false), end["isDragging"])
                    assertEquals(WhiskerValue.Bool(cancel), end["isDragCancelled"])
                    assertEquals(WhiskerValue.Float(0.0), end[if (horizontal) "deltaX" else "deltaY"])
                    val velocity = (end[if (horizontal) "velocityX" else "velocityY"] as WhiskerValue.Float).value
                    if (cancel) assertEquals(0.0, velocity, 0.0)
                    else assertEquals((if (atEdge) -2500.0 else 2500.0) / activity.resources.displayMetrics.density, velocity, 150.0)
                    val positionKey = if (horizontal) "scrollLeft" else "scrollTop"
                    val deltaKey = if (horizontal) "deltaX" else "deltaY"
                    var previous = 0.0
                    for (detail in events) {
                        val position = (detail[positionKey] as WhiskerValue.Float).value
                        assertEquals(position - previous, (detail[deltaKey] as WhiskerValue.Float).value, 0.001)
                        previous = position
                        if (atEdge) assertEquals("edge gesture must not move content", 0.0, position, 0.001)
                    }
                }
            }
        }
    }

    @Test fun verticalEdgeDrag() = gesture(false, atEdge = true)
    @Test fun horizontalEdgeDrag() = gesture(true, atEdge = true)

    @Test fun programmaticScrollReportsDeltaWithoutDragging() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val intent = Intent(instrumentation.context, ScrollCancellationActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ActivityScenario.launch<ScrollCancellationActivity>(intent).use { scenario ->
            instrumentation.waitForIdleSync()
            val events = mutableListOf<Map<String, WhiskerValue>>()
            scenario.onActivity { activity ->
                activity.scroll.installWhiskerEventSink { _, value -> events.add((value as WhiskerValue.Map).value) }
                activity.scroll.scrollToLogicalOffset(100.0, false)
            }
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity -> activity.scroll.scrollToLogicalOffset(60.0, false) }
            instrumentation.waitForIdleSync()
            scenario.onActivity {
                assertEquals(2, events.size)
                assertEquals(100.0, (events[0]["deltaY"] as WhiskerValue.Float).value, 1.0)
                assertEquals(-40.0, (events[1]["deltaY"] as WhiskerValue.Float).value, 1.0)
                for (event in events) {
                    assertEquals(WhiskerValue.Bool(false), event["isDragging"])
                    assertEquals(WhiskerValue.Float(0.0), event["velocityY"])
                }
            }
        }
    }

    @Test fun verticalDrag() = gesture(false)
    @Test fun horizontalDrag() = gesture(true)
    @Test fun interceptClickableChild() = gesture(false, clickable = true)
    @Test fun interceptHorizontalClickableChild() = gesture(true, clickable = true)
    @Test fun verticalCancel() = gesture(false, cancel = true)
    @Test fun horizontalCancel() = gesture(true, cancel = true)
    @Test fun tapDoesNotEmitDrag() = gesture(false, tap = true)
}
