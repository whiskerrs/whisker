package rs.whisker.runtime

import android.content.Intent
import android.os.SystemClock
import android.view.MotionEvent
import androidx.swiperefreshlayout.widget.SwipeRefreshLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ScrollRefreshTest {
    private fun ScrollCancellationActivity.refreshLayout(): SwipeRefreshLayout =
        scroll.getChildAt(0) as SwipeRefreshLayout

    private fun launch(block: (ActivityScenario<ScrollCancellationActivity>) -> Unit) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val intent = Intent(instrumentation.context, ScrollCancellationActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ActivityScenario.launch<ScrollCancellationActivity>(intent).use { scenario ->
            instrumentation.waitForIdleSync()
            block(scenario)
        }
    }

    @Test
    fun refreshIsAvailableOnlyWhileEnabledVerticalAndScrollable() = launch { scenario ->
        scenario.onActivity { activity ->
            val refresh = activity.refreshLayout()
            assertFalse(refresh.isEnabled)

            activity.scroll.setRefreshEnabled(true)
            assertTrue(refresh.isEnabled)
            assertTrue(activity.nativeScroller.isNestedScrollingEnabled)

            activity.scroll.setUserScrollEnabled(false)
            assertFalse(refresh.isEnabled)
            activity.scroll.setUserScrollEnabled(true)
            assertTrue(refresh.isEnabled)

            activity.scroll.setScrollOrientation("horizontal")
            assertFalse(refresh.isEnabled)
            activity.scroll.setScrollOrientation("vertical")
            assertTrue(refresh.isEnabled)

            activity.scroll.setRefreshEnabled(false)
            assertFalse(refresh.isEnabled)
            assertFalse(activity.nativeScroller.isNestedScrollingEnabled)
        }
    }

    @Test
    fun refreshingPropDrivesTheIndicatorOnlyWhileEnabled() = launch { scenario ->
        scenario.onActivity { activity ->
            val refresh = activity.refreshLayout()
            activity.scroll.setRefreshing(true)
            assertFalse("a disabled control shows no indicator", refresh.isRefreshing)

            activity.scroll.setRefreshEnabled(true)
            assertTrue("a pending request starts once enabled", refresh.isRefreshing)

            activity.scroll.setRefreshing(false)
            assertFalse(refresh.isRefreshing)
            activity.scroll.setRefreshing(true)
            assertTrue(refresh.isRefreshing)

            activity.scroll.setRefreshEnabled(false)
            assertFalse(refresh.isRefreshing)
        }
    }

    @Test
    fun pullingDownAtTheTopEmitsRefresh() = launch { scenario ->
        val refreshed = CountDownLatch(1)
        val events = mutableListOf<String>()
        scenario.onActivity { activity ->
            activity.scroll.setRefreshEnabled(true)
            activity.scroll.installWhiskerEventSink { name, value ->
                events.add(name)
                if (name == "refresh") {
                    assertEquals(WhiskerValue.Map(emptyMap()), value)
                    refreshed.countDown()
                }
            }
            val density = activity.resources.displayMetrics.density
            val now = SystemClock.uptimeMillis()
            fun send(action: Int, time: Long, y: Float) {
                val event = MotionEvent.obtain(now, now + time, action, 100f, y, 0)
                activity.scroll.dispatchTouchEvent(event)
                event.recycle()
            }
            send(MotionEvent.ACTION_DOWN, 0, 10f)
            for (step in 1..10) send(MotionEvent.ACTION_MOVE, step * 16L, 10f + step * 40f * density)
            send(MotionEvent.ACTION_UP, 176, 10f + 400f * density)
        }
        assertTrue("pull past the threshold must emit refresh", refreshed.await(5, TimeUnit.SECONDS))
        scenario.onActivity { activity ->
            assertTrue(activity.refreshLayout().isRefreshing)
            assertEquals(1, events.count { it == "refresh" })
            assertEquals("the pull must not move content", 0, activity.offset)
        }
    }

    @Test
    fun disabledRefreshLeavesTheTopEdgeDragToTheScroller() = launch { scenario ->
        scenario.onActivity { activity ->
            val events = mutableListOf<String>()
            activity.scroll.installWhiskerEventSink { name, _ -> events.add(name) }
            val now = SystemClock.uptimeMillis()
            fun send(action: Int, time: Long, y: Float) {
                val event = MotionEvent.obtain(now, now + time, action, 100f, y, 0)
                activity.scroll.dispatchTouchEvent(event)
                event.recycle()
            }
            send(MotionEvent.ACTION_DOWN, 0, 10f)
            for (step in 1..10) send(MotionEvent.ACTION_MOVE, step * 16L, 10f + step * 60f)
            send(MotionEvent.ACTION_UP, 176, 610f)
            assertFalse(events.contains("refresh"))
            assertFalse(activity.refreshLayout().isRefreshing)
        }
    }
}
